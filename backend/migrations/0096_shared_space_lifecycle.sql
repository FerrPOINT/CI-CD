-- Additive v2 Space participant; historical v1 bindings/receipts are preserved.
-- New spaces are barriers only: no implicit Git group, repository or CI project.
CREATE TABLE forge_space_catalog (
    registry_instance_id uuid NOT NULL CHECK(registry_instance_id <> '00000000-0000-0000-0000-000000000000'),
    space_id uuid NOT NULL CHECK(space_id <> '00000000-0000-0000-0000-000000000000'),
    group_id uuid UNIQUE REFERENCES git_groups(id) ON DELETE RESTRICT,
    revision bigint NOT NULL CHECK(revision > 0),
    state text NOT NULL CHECK(state IN ('active','archiving','archived')),
    operation_id uuid NOT NULL,
    command jsonb NOT NULL CHECK(COALESCE(command->>'schema_version'='2',false)),
    PRIMARY KEY(registry_instance_id,space_id),
    CHECK(COALESCE(command->'space'->>'registry_instance_id'=registry_instance_id::text AND command->'space'->>'space_id'=space_id::text AND command->>'operation_id'=operation_id::text AND command->>'revision'=revision::text,false))
);
CREATE TABLE forge_space_owner_operations (
    operation_id uuid PRIMARY KEY,
    registry_instance_id uuid NOT NULL,
    space_id uuid NOT NULL,
    revision bigint NOT NULL CHECK(revision>0),
    command jsonb NOT NULL,
    readback jsonb NOT NULL,
    owner_issuer text NOT NULL CHECK(btrim(owner_issuer)<>''),
    owner_subject text NOT NULL CHECK(btrim(owner_subject)<>''),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(registry_instance_id,space_id,revision),
    FOREIGN KEY(registry_instance_id,space_id) REFERENCES forge_space_catalog ON DELETE RESTRICT
);

-- Both versions serialize the same public ID, including direct legacy writes.
CREATE FUNCTION forge_legacy_space_owner_fence() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE registry uuid; space uuid;
BEGIN
    IF TG_OP='UPDATE' AND (NEW.registry_instance_id,NEW.namespace_id,NEW.resource_id) IS DISTINCT FROM (OLD.registry_instance_id,OLD.namespace_id,OLD.resource_id) THEN
        RAISE EXCEPTION 'namespace_binding_identity_immutable' USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN registry:=OLD.registry_instance_id; space:=OLD.namespace_id;
    ELSE registry:=NEW.registry_instance_id; space:=NEW.namespace_id; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('shared-space:'||registry::text||':'||space::text,0));
    IF EXISTS(SELECT 1 FROM forge_space_catalog WHERE registry_instance_id=registry AND space_id=space) THEN
        RAISE EXCEPTION 'shared_space_requires_owner_v2' USING ERRCODE='42501';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER shared_space_owner_fence BEFORE INSERT OR UPDATE OR DELETE ON forge_namespace_bindings FOR EACH ROW EXECUTE FUNCTION forge_legacy_space_owner_fence();

CREATE FUNCTION forge_space_catalog_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' OR (NEW.registry_instance_id,NEW.space_id,NEW.group_id) IS DISTINCT FROM (OLD.registry_instance_id,OLD.space_id,OLD.group_id) THEN
        RAISE EXCEPTION 'shared_space_identity_must_be_preserved' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER shared_space_identity BEFORE UPDATE OR DELETE ON forge_space_catalog FOR EACH ROW EXECUTE FUNCTION forge_space_catalog_identity();

-- Keep lock order: v1 binding first, then its optional v2 catalog row. Owner v2
-- takes the same group advisory lock and exclusive binding lock before catalog.
CREATE FUNCTION forge_effective_group_state(gid uuid) RETURNS text LANGUAGE plpgsql AS $$
DECLARE legacy text; current_state text;
BEGIN
    SELECT state INTO legacy FROM forge_namespace_bindings WHERE resource_id=gid FOR SHARE;
    SELECT state INTO current_state FROM forge_space_catalog WHERE group_id=gid FOR SHARE;
    RETURN COALESCE(current_state,legacy);
END $$;

CREATE OR REPLACE FUNCTION forge_namespace_start_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE pid uuid; gid uuid; lifecycle text;
BEGIN
    IF TG_TABLE_NAME='pipelines' THEN pid := NEW.project_id;
    ELSIF TG_TABLE_NAME='job_leases' THEN
        IF TG_OP='UPDATE' THEN
            IF NEW.lease_status<>'active' OR (OLD.lease_status='active' AND (OLD.acknowledged_at IS NOT NULL OR NEW.acknowledged_at IS NULL)) THEN RETURN NEW; END IF;
        END IF;
        SELECT p.project_id INTO pid FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id WHERE j.id=NEW.job_id;
    ELSE
        IF NEW.status <> 'running' OR (TG_OP='UPDATE' AND OLD.status='running') THEN RETURN NEW; END IF;
        SELECT p.project_id INTO pid FROM stages s JOIN pipelines p ON p.id=s.pipeline_id WHERE s.id=NEW.stage_id;
    END IF;
    SELECT r.group_id INTO gid FROM projects p JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=pid;
    lifecycle := forge_effective_group_state(gid);
    IF (lifecycle IS NOT NULL AND lifecycle <> 'active') OR (lifecycle IS NULL AND EXISTS(SELECT 1 FROM git_groups WHERE id=gid AND namespace_managed)) THEN RAISE EXCEPTION 'namespace_resource_read_only' USING ERRCODE='42501'; END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION forge_namespace_mutation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE data jsonb; gids uuid[] := '{}'; gid uuid; lifecycle text;
BEGIN
    IF TG_TABLE_NAME='git_groups' AND TG_OP='UPDATE'
       AND current_setting('pdlc.forge.space_owner',true)='admin_v2'
       AND to_jsonb(NEW)-'name'=to_jsonb(OLD)-'name' THEN RETURN NEW; END IF;
    IF TG_TABLE_NAME='git_groups' AND TG_OP='UPDATE' THEN
        IF OLD.namespace_managed AND NOT NEW.namespace_managed THEN RAISE EXCEPTION 'namespace_marker_immutable' USING ERRCODE='42501'; END IF;
        IF NOT OLD.namespace_managed AND NEW.namespace_managed AND to_jsonb(OLD)-'namespace_managed'=to_jsonb(NEW)-'namespace_managed' AND EXISTS(SELECT 1 FROM forge_namespace_bindings WHERE resource_id=NEW.id) THEN RETURN NEW; END IF;
    END IF;
    FOR data IN SELECT value FROM jsonb_array_elements(CASE TG_OP WHEN 'INSERT' THEN jsonb_build_array(to_jsonb(NEW)) WHEN 'DELETE' THEN jsonb_build_array(to_jsonb(OLD)) ELSE jsonb_build_array(to_jsonb(OLD),to_jsonb(NEW)) END)
    LOOP
        IF TG_TABLE_NAME='git_groups' THEN gid := (data->>'id')::uuid;
        ELSIF TG_TABLE_NAME='repository_catalog' THEN gid := (data->>'group_id')::uuid;
        ELSIF TG_TABLE_NAME='repositories' THEN SELECT group_id INTO gid FROM repository_catalog WHERE id=(data->>'id')::uuid;
        ELSIF data ? 'repository_id' THEN SELECT group_id INTO gid FROM repository_catalog WHERE id=(data->>'repository_id')::uuid;
        ELSIF data ? 'project_id' THEN SELECT r.group_id INTO gid FROM projects p JOIN repository_catalog r ON r.id=p.repository_id WHERE p.id=(data->>'project_id')::uuid;
        END IF;
        gids := array_append(gids,gid);
    END LOOP;
    IF EXISTS(SELECT 1 FROM git_groups g WHERE g.id=ANY(gids) AND g.namespace_managed AND NOT EXISTS(SELECT 1 FROM forge_namespace_bindings b WHERE b.resource_id=g.id)) THEN
        RAISE EXCEPTION 'namespace_projection_missing' USING ERRCODE='42501';
    END IF;
    FOR lifecycle IN SELECT forge_effective_group_state(id) FROM (SELECT DISTINCT unnest(gids) AS id) affected_groups WHERE id IS NOT NULL ORDER BY id
    LOOP
        IF lifecycle <> 'active' OR (TG_OP='DELETE' AND TG_TABLE_NAME IN ('git_groups','repository_catalog','repository_aliases','repositories')) THEN RAISE EXCEPTION 'namespace_resource_read_only' USING ERRCODE='42501'; END IF;
    END LOOP;
    IF TG_OP='UPDATE' AND TG_TABLE_NAME='repository_catalog' THEN
        IF OLD.group_id IS NOT NULL AND (NEW.group_id IS DISTINCT FROM OLD.group_id OR NEW.slug<>OLD.slug OR NEW.storage_name IS DISTINCT FROM OLD.storage_name OR NEW.kind<>OLD.kind) THEN RAISE EXCEPTION 'repository_identity_immutable' USING ERRCODE='42501'; END IF;
    END IF;
    IF TG_OP='UPDATE' AND TG_TABLE_NAME='projects' THEN
        IF OLD.repository_id IS NOT NULL AND NEW.repository_id IS DISTINCT FROM OLD.repository_id THEN RAISE EXCEPTION 'delivery_repository_immutable' USING ERRCODE='42501'; END IF;
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;

-- V2 metadata is Admin-owned. Resource mutation lifecycle guards above remain
-- active even during an authorized rename; no role/data ownership is transferred.
CREATE FUNCTION forge_shared_group_metadata_owner() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM forge_space_catalog WHERE group_id=OLD.id)
       AND current_setting('pdlc.forge.space_owner',true) IS DISTINCT FROM 'admin_v2'
       AND (TG_OP='DELETE' OR (NEW.slug,NEW.name) IS DISTINCT FROM (OLD.slug,OLD.name)) THEN
        RAISE EXCEPTION 'shared_space_metadata_requires_admin_owner' USING ERRCODE='42501';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER shared_group_metadata_owner BEFORE UPDATE OR DELETE ON git_groups FOR EACH ROW EXECUTE FUNCTION forge_shared_group_metadata_owner();
