-- Additive, independent of the pending SDLC migrations 0039/0040.
CREATE TABLE git_groups (
    id uuid PRIMARY KEY,
    namespace_managed boolean NOT NULL DEFAULT false,
    slug text NOT NULL UNIQUE CHECK(slug ~ '^[a-z][a-z0-9-]{0,63}$'),
    name text NOT NULL CHECK(btrim(name) <> ''),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE forge_namespace_bindings (
    resource_id uuid PRIMARY KEY REFERENCES git_groups(id) ON DELETE RESTRICT,
    registry_instance_id uuid NOT NULL,
    namespace_id uuid NOT NULL,
    generation bigint NOT NULL CHECK(generation > 0),
    state text NOT NULL CHECK(state IN ('active','archived')),
    command jsonb NOT NULL,
    UNIQUE(registry_instance_id,namespace_id)
);
CREATE TABLE repository_catalog (
    id uuid PRIMARY KEY REFERENCES repositories(id) ON DELETE RESTRICT,
    group_id uuid REFERENCES git_groups(id) ON DELETE RESTRICT,
    slug text NOT NULL,
    kind text NOT NULL CHECK(kind IN ('hosted','external')),
    storage_name text UNIQUE REFERENCES repositories(name) ON DELETE RESTRICT,
    external_url text,
    provider_refs jsonb NOT NULL DEFAULT '{}'::jsonb,
    ready boolean NOT NULL DEFAULT true,
    create_command jsonb,
    CHECK((kind='hosted' AND storage_name IS NOT NULL AND external_url IS NULL) OR (kind='external' AND storage_name IS NULL AND external_url IS NOT NULL)),
    UNIQUE(group_id,slug)
);
INSERT INTO repository_catalog(id,slug,kind,storage_name) SELECT id,name,'hosted',name FROM repositories;
CREATE TABLE repository_aliases (
    alias text PRIMARY KEY,
    repository_id uuid NOT NULL REFERENCES repository_catalog(id) ON DELETE RESTRICT
);
INSERT INTO repository_aliases(alias,repository_id) SELECT storage_name,id FROM repository_catalog;
CREATE FUNCTION forge_legacy_repository_catalog() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO repository_catalog(id,slug,kind,storage_name) VALUES(NEW.id,NEW.name,'hosted',NEW.name);
    INSERT INTO repository_aliases(alias,repository_id) VALUES(NEW.name,NEW.id);
    RETURN NEW;
END $$;
CREATE TRIGGER legacy_repository_catalog AFTER INSERT ON repositories FOR EACH ROW EXECUTE FUNCTION forge_legacy_repository_catalog();
ALTER TABLE pull_requests ADD COLUMN repository_id uuid REFERENCES repository_catalog(id) ON DELETE RESTRICT;
UPDATE pull_requests p SET repository_id=r.id FROM repository_catalog r WHERE p.repository_name=r.storage_name;
CREATE UNIQUE INDEX pull_requests_repository_id_number ON pull_requests(repository_id,number) WHERE repository_id IS NOT NULL;
CREATE TABLE repository_pr_counters (
    repository_id uuid PRIMARY KEY REFERENCES repository_catalog(id) ON DELETE RESTRICT,
    high_water_mark integer NOT NULL CHECK(high_water_mark >= 0)
);
INSERT INTO repository_pr_counters(repository_id,high_water_mark)
SELECT r.id,COALESCE(MAX(p.number),0) FROM repository_catalog r LEFT JOIN pull_requests p ON p.repository_id=r.id GROUP BY r.id;
CREATE FUNCTION allocate_repository_pr_number(repository uuid) RETURNS integer LANGUAGE sql AS $$
    INSERT INTO repository_pr_counters(repository_id,high_water_mark) VALUES(repository,1)
    ON CONFLICT(repository_id) DO UPDATE SET high_water_mark=repository_pr_counters.high_water_mark+1 RETURNING high_water_mark;
$$;
ALTER TABLE projects ADD COLUMN repository_id uuid REFERENCES repository_catalog(id) ON DELETE RESTRICT;
-- Delivery links require an explicit mapping. No URL-tail or same-name backfill.
CREATE FUNCTION forge_repository_id_projection() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.repository_id IS NULL THEN
        SELECT id INTO NEW.repository_id FROM repository_catalog WHERE storage_name=NEW.repository_name;
    ELSIF NOT EXISTS(SELECT 1 FROM repository_catalog WHERE id=NEW.repository_id AND storage_name=NEW.repository_name) THEN
        RAISE EXCEPTION 'repository_identity_mismatch';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER pull_request_repository_projection BEFORE INSERT OR UPDATE ON pull_requests FOR EACH ROW EXECUTE FUNCTION forge_repository_id_projection();
CREATE FUNCTION forge_namespace_start_guard() RETURNS trigger LANGUAGE plpgsql AS $$
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
    SELECT state INTO lifecycle FROM forge_namespace_bindings WHERE resource_id=gid FOR SHARE;
    IF (lifecycle IS NOT NULL AND lifecycle <> 'active') OR (lifecycle IS NULL AND EXISTS(SELECT 1 FROM git_groups WHERE id=gid AND namespace_managed)) THEN RAISE EXCEPTION 'namespace_resource_read_only' USING ERRCODE='42501'; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER namespace_pipeline_start_guard BEFORE INSERT ON pipelines FOR EACH ROW EXECUTE FUNCTION forge_namespace_start_guard();
CREATE TRIGGER namespace_job_start_guard BEFORE INSERT OR UPDATE ON jobs FOR EACH ROW EXECUTE FUNCTION forge_namespace_start_guard();
CREATE TRIGGER namespace_lease_start_guard BEFORE INSERT OR UPDATE ON job_leases FOR EACH ROW EXECUTE FUNCTION forge_namespace_start_guard();

ALTER TABLE forge_namespace_bindings ADD CONSTRAINT namespace_projection_consistent CHECK (COALESCE((
    command->>'schema_version'='1' AND command->'namespace'->>'registry_instance_id'=registry_instance_id::text
    AND command->'namespace'->>'namespace_id'=namespace_id::text AND command->'resource'->>'resource_id'=resource_id::text
    AND command->'resource'->>'kind'='git_group' AND command->>'generation'=generation::text AND command->>'state'=state
    AND command ? 'operation_id' AND command->'resource' ? 'instance_id'
    AND jsonb_typeof(command->'namespace')='object' AND jsonb_typeof(command->'resource')='object'
),false)
);

CREATE TABLE pull_task_links (
 pull_request_id uuid NOT NULL REFERENCES pull_requests(id) ON DELETE RESTRICT,
 repository_id uuid NOT NULL REFERENCES repository_catalog(id) ON DELETE RESTRICT,
 tracker_instance_id uuid NOT NULL, task_id uuid NOT NULL,
 registry_instance_id uuid NOT NULL, namespace_id uuid NOT NULL,
 source_commit_sha text NOT NULL CHECK(source_commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
 task_key_snapshot text NOT NULL,
 created_by_user_id uuid NOT NULL REFERENCES users(id),
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(pull_request_id,tracker_instance_id,task_id)
);
CREATE FUNCTION forge_task_link_scope() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM pull_requests p JOIN repository_catalog r ON r.id=p.repository_id JOIN forge_namespace_bindings b ON b.resource_id=r.group_id
   WHERE p.id=NEW.pull_request_id AND p.repository_id=NEW.repository_id AND b.registry_instance_id=NEW.registry_instance_id AND b.namespace_id=NEW.namespace_id) THEN
   RAISE EXCEPTION 'foreign_task_link_projection' USING ERRCODE='42501';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER namespace_task_link_scope BEFORE INSERT OR UPDATE ON pull_task_links FOR EACH ROW EXECUTE FUNCTION forge_task_link_scope();

-- Legacy metadata routes enforce the same lifecycle and cannot detach identities.
CREATE TABLE repository_attach_operations (
 operation_id uuid PRIMARY KEY,
 repository_id uuid NOT NULL UNIQUE REFERENCES repository_catalog(id) ON DELETE RESTRICT,
 actor_id uuid NOT NULL,
 command jsonb NOT NULL,
 readback jsonb NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE FUNCTION forge_namespace_mutation_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE data jsonb; gids uuid[] := '{}'; gid uuid; lifecycle text;
BEGIN
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
    FOR lifecycle IN SELECT state FROM forge_namespace_bindings WHERE resource_id=ANY(gids) ORDER BY resource_id FOR SHARE
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
DO $$ DECLARE table_name text; BEGIN
    FOREACH table_name IN ARRAY ARRAY['git_groups','repository_catalog','repository_aliases','repositories','repository_pr_counters','pull_requests','pull_task_links','projects','schedules','webhooks','project_secrets','notification_configs','environments']
    LOOP
        EXECUTE format('CREATE TRIGGER namespace_mutation_guard BEFORE INSERT OR UPDATE OR DELETE ON %I FOR EACH ROW EXECUTE FUNCTION forge_namespace_mutation_guard()',table_name);
    END LOOP;
END $$;

CREATE FUNCTION forge_mark_namespace_resource() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 UPDATE git_groups SET namespace_managed=true WHERE id=NEW.resource_id AND NOT namespace_managed;
 RETURN NEW;
END $$;
CREATE TRIGGER forge_namespace_resource_marker AFTER INSERT ON forge_namespace_bindings FOR EACH ROW EXECUTE FUNCTION forge_mark_namespace_resource();

-- Legacy writes may not rely on an unreadable strict-v1 owner projection.
ALTER TABLE forge_namespace_bindings ADD CONSTRAINT namespace_projection_shape CHECK (COALESCE((
    command - ARRAY['schema_version','namespace','resource','operation_id','generation','state','create_spec'] = '{}'::jsonb
    AND command ?& ARRAY['schema_version','namespace','resource','operation_id','generation','state']
    AND jsonb_typeof(command->'schema_version')='number' AND jsonb_typeof(command->'generation')='number'
    AND (command->'namespace') - ARRAY['registry_instance_id','namespace_id'] = '{}'::jsonb
    AND (command->'resource') - ARRAY['kind','instance_id','resource_id'] = '{}'::jsonb
    AND command->>'operation_id' ~ '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
    AND command->'resource'->>'instance_id' ~ '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
    AND command->>'operation_id'<>'00000000-0000-0000-0000-000000000000'
    AND command->'resource'->>'instance_id'<>'00000000-0000-0000-0000-000000000000'
    AND registry_instance_id<>'00000000-0000-0000-0000-000000000000'::uuid
    AND namespace_id<>'00000000-0000-0000-0000-000000000000'::uuid
    AND resource_id<>'00000000-0000-0000-0000-000000000000'::uuid
    AND (NOT command ? 'create_spec' OR command->'create_spec'='null'::jsonb OR jsonb_typeof(command->'create_spec')='object')
),false));
