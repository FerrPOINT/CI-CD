-- Tracker owns project presentation. This projection never grants resource ownership.
CREATE TABLE forge_workspace_projects (
 registry_instance_id uuid NOT NULL,
 namespace_id uuid NOT NULL,
 tracker_instance_id uuid NOT NULL,
 tracker_project_id uuid NOT NULL,
 generation bigint NOT NULL CHECK(generation > 0),
 name text NOT NULL,
 project_key text NOT NULL,
 state text NOT NULL CHECK(state IN ('active','archived')),
 observed_at timestamptz NOT NULL,
 PRIMARY KEY(registry_instance_id,namespace_id),
 UNIQUE(tracker_instance_id,tracker_project_id)
);
CREATE TABLE repository_push_configs (
 repository_id uuid PRIMARY KEY REFERENCES repository_catalog(id) ON DELETE RESTRICT,
 configuration_id uuid REFERENCES projects(id) ON DELETE RESTRICT
);
CREATE TABLE repository_push_operations (
 repository_id uuid NOT NULL REFERENCES repository_catalog(id) ON DELETE RESTRICT,
 trigger_key text NOT NULL,
 configuration_id uuid REFERENCES projects(id) ON DELETE RESTRICT,
 PRIMARY KEY(repository_id,trigger_key)
);
-- Existing hook receipts keep their original configuration across later policy changes.
INSERT INTO repository_push_operations(repository_id,trigger_key,configuration_id)
 SELECT p.repository_id,t.idempotency_key,CASE WHEN count(DISTINCT p.id)=1 THEN (array_agg(DISTINCT p.id))[1] ELSE NULL END FROM pipeline_triggers t JOIN projects p ON p.id=t.project_id
 WHERE p.repository_id IS NOT NULL AND t.source='git-push'
 GROUP BY p.repository_id,t.idempotency_key
 ON CONFLICT DO NOTHING;
-- Preserve only already explicit single-config mappings. Ambiguous mappings stay unresolved.
INSERT INTO repository_push_configs(repository_id,configuration_id)
 SELECT repository_id,(array_agg(id))[1] FROM projects WHERE repository_id IS NOT NULL
 GROUP BY repository_id HAVING count(*)=1;
CREATE TABLE delivery_configuration_operations (
 operation_id uuid PRIMARY KEY,
 repository_id uuid NOT NULL REFERENCES repository_catalog(id) ON DELETE RESTRICT,
 actor_id uuid NOT NULL,
 payload jsonb NOT NULL,
 configuration_id uuid NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
 readback jsonb NOT NULL
);
CREATE INDEX projects_repository_id_idx ON projects(repository_id);
ALTER TABLE projects DROP CONSTRAINT projects_name_key;
CREATE UNIQUE INDEX delivery_configuration_repository_name ON projects(repository_id,name) WHERE repository_id IS NOT NULL;
CREATE UNIQUE INDEX legacy_unbound_configuration_name ON projects(name) WHERE repository_id IS NULL;
CREATE TABLE forge_workspace_catalog_sync(id boolean PRIMARY KEY CHECK(id), observed_at timestamptz NOT NULL);
CREATE FUNCTION forge_push_configuration_identity() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.configuration_id IS NOT NULL AND NOT EXISTS(
  SELECT 1 FROM projects WHERE id=NEW.configuration_id AND repository_id=NEW.repository_id
 ) THEN RAISE EXCEPTION 'push_configuration_repository_mismatch' USING ERRCODE='23514'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER push_configuration_identity BEFORE INSERT OR UPDATE ON repository_push_configs
 FOR EACH ROW EXECUTE FUNCTION forge_push_configuration_identity();
-- API shims must not permit deleting history or rewriting a bound checkout identity.
CREATE FUNCTION forge_configuration_history_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' THEN
  IF EXISTS(SELECT 1 FROM pipelines WHERE project_id=OLD.id)
   OR EXISTS(SELECT 1 FROM environments e JOIN deployments d ON d.environment_id=e.id WHERE e.project_id=OLD.id)
   OR EXISTS(SELECT 1 FROM repository_catalog r JOIN git_groups g ON g.id=r.group_id WHERE r.id=OLD.repository_id AND g.namespace_managed)
  THEN RAISE EXCEPTION 'delivery_configuration_history_protected' USING ERRCODE='42501'; END IF;
  RETURN OLD;
 END IF;
 IF OLD.repository_id IS NOT NULL AND NEW.repository_url IS DISTINCT FROM OLD.repository_url
 THEN RAISE EXCEPTION 'delivery_checkout_identity_immutable' USING ERRCODE='42501'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER configuration_history_guard BEFORE DELETE OR UPDATE ON projects
 FOR EACH ROW EXECUTE FUNCTION forge_configuration_history_guard();
