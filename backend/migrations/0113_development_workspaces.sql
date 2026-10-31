CREATE TABLE development_workspaces (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    repository_id uuid NOT NULL REFERENCES repository_catalog(id) ON DELETE RESTRICT,
    environment_id uuid NOT NULL UNIQUE REFERENCES environments(id) ON DELETE RESTRICT,
    environment_hash text NOT NULL CHECK (environment_hash ~ '^[a-f0-9]{64}$'),
    tracker_instance_id uuid NOT NULL,
    tracker_project_id uuid NOT NULL,
    FOREIGN KEY(tracker_instance_id,tracker_project_id) REFERENCES forge_workspace_projects(tracker_instance_id,tracker_project_id) ON DELETE RESTRICT,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    profile_id uuid NOT NULL,
    target_id uuid NOT NULL UNIQUE,
    catalog_target_id uuid NOT NULL UNIQUE,
    public_origin text,
    catalog_hash text NOT NULL CHECK (catalog_hash ~ '^[a-f0-9]{64}$'),
    profile_hash text NOT NULL CHECK (profile_hash ~ '^[a-f0-9]{64}$'),
    profile jsonb NOT NULL CHECK (jsonb_typeof(profile)='object'),
    local_preparation jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(local_preparation)='object'),
    sources jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(sources)='array'),
    preparation text NOT NULL DEFAULT 'unprepared' CHECK (preparation IN ('unprepared','prepared','degraded','unknown')),
    desired_runtime text NOT NULL DEFAULT 'stopped' CHECK (desired_runtime IN ('stopped','running')),
    runtime text NOT NULL DEFAULT 'unknown' CHECK (runtime IN ('stopped','starting','running','stopping','degraded','unknown')),
    observation jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(observation)='object'),
    observed_at timestamptz,
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_by uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id,name)
);

CREATE TABLE development_workspace_sessions (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES development_workspaces(id) ON DELETE RESTRICT,
    actor_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    owner_subject uuid NOT NULL,
    task_id uuid NOT NULL,
    task_ref text NOT NULL CHECK (length(task_ref) BETWEEN 1 AND 128),
    sources jsonb NOT NULL DEFAULT '[]',
    started_at timestamptz NOT NULL DEFAULT now(),
    ended_at timestamptz,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);
CREATE UNIQUE INDEX development_workspace_single_writer
    ON development_workspace_sessions(workspace_id) WHERE ended_at IS NULL;

CREATE TABLE development_workspace_operations (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES development_workspaces(id) ON DELETE RESTRICT,
    actor_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    owner_subject uuid NOT NULL,
    request_hash text NOT NULL CHECK (request_hash ~ '^[a-f0-9]{64}$'),
    execution_plan jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(execution_plan)='object'),
    request jsonb NOT NULL CHECK (jsonb_typeof(request)='object'),
    expected_version bigint NOT NULL CHECK (expected_version > 0),
    state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued','running','succeeded','failed','unknown')),
    worker_id uuid,
    effect_id uuid,
    pipeline_id uuid REFERENCES pipelines(id) ON DELETE RESTRICT,
    recovery_actor_id uuid REFERENCES users(id) ON DELETE RESTRICT,
    recovery_owner_subject uuid,
    recovery_requested_at timestamptz,
    reconcile_only boolean NOT NULL DEFAULT false,
    result jsonb,
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    CHECK ((state IN ('succeeded','failed')) = (finished_at IS NOT NULL))
);
CREATE UNIQUE INDEX development_workspace_single_operation
    ON development_workspace_operations(workspace_id) WHERE state IN ('queued','running','unknown');
CREATE INDEX development_workspaces_project ON development_workspaces(project_id,created_at,id);
CREATE INDEX development_workspace_operation_history ON development_workspace_operations(workspace_id,created_at,id);

CREATE FUNCTION guard_development_workspace_history() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'development workspace history is retained' USING ERRCODE='23514';
    END IF;
    IF ROW(OLD.id,OLD.workspace_id,OLD.actor_id,OLD.owner_subject,OLD.request_hash,OLD.request,OLD.execution_plan,OLD.expected_version,OLD.created_at)
       IS DISTINCT FROM ROW(NEW.id,NEW.workspace_id,NEW.actor_id,NEW.owner_subject,NEW.request_hash,NEW.request,NEW.execution_plan,NEW.expected_version,NEW.created_at)
       OR OLD.state IN ('succeeded','failed') THEN
        RAISE EXCEPTION 'development workspace operation identity or terminal result is immutable' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER development_workspace_immutable_operations BEFORE UPDATE OR DELETE ON development_workspace_operations
    FOR EACH ROW EXECUTE FUNCTION guard_development_workspace_history();

SELECT forge_install_human_request_clocks();

CREATE OR REPLACE FUNCTION forge_identity_row_refs(source text,document jsonb)
RETURNS TABLE(issuer text,subject text,unresolved boolean) LANGUAGE plpgsql STABLE AS $$
DECLARE keys text[];key text;legacy text;refs jsonb;
BEGIN
 SELECT l.issuer INTO legacy FROM identity_legacy_issuer l WHERE singleton;
 CASE source
 WHEN 'users' THEN
  IF document->'central_sub' IS NOT NULL AND document->'central_sub'<>'null'::jsonb THEN
   RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'central_sub',NULL,legacy,true);
  ELSE RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>'id');END IF;
 WHEN 'user_credentials','sessions','api_tokens','project_memberships','tenant_memberships','notification_preferences','forge_confirmed_user_roles' THEN keys:=ARRAY['user_id'];
 WHEN 'service_accounts','tenants' THEN keys:=ARRAY['created_by'];
 WHEN 'pull_task_links' THEN keys:=ARRAY['created_by_user_id'];
 WHEN 'repository_attach_operations','delivery_configuration_operations','repository_sync_operations' THEN
  RETURN QUERY SELECT * FROM forge_identity_alias('local_principal_id',document->>'actor_id');
 WHEN 'pull_requests' THEN RETURN QUERY SELECT * FROM forge_identity_alias('username',document->>'created_by');
 WHEN 'audit_log','releases','deployment_approvals' THEN
  refs:=document->'identity_refs';
  IF refs IS NOT NULL AND refs<>'null'::jsonb THEN RETURN QUERY SELECT * FROM forge_identity_typed_refs(refs);
  ELSIF source='releases' THEN RETURN QUERY SELECT * FROM forge_identity_alias('legacy_actor',document->>'created_by');
  ELSE RETURN QUERY SELECT * FROM forge_identity_alias('legacy_actor',document->>'actor');END IF;
  IF source='audit_log' AND document->>'resource_type'='user' THEN RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>'resource_id');END IF;
 WHEN 'forge_space_owner_operations' THEN
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',document->'owner_issuer',NULL,true);
 WHEN 'task_pipeline_intents' THEN
  IF document->'delegated_credential' IS NOT NULL AND document->'delegated_credential'<>'null'::jsonb THEN
   RETURN QUERY SELECT * FROM forge_identity_central_ref(document#>'{intent,holderSubject}',NULL,legacy,true);
   RETURN QUERY SELECT * FROM forge_identity_central_ref(document#>'{delegated_credential,subject}',NULL,legacy,true);
  END IF;
 WHEN 'task_credential_successors' THEN
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document#>'{credential,subject}',NULL,legacy,true);
 WHEN 'sdlc_workspace_operations' THEN
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'delegated_subject',NULL,legacy,false);
 WHEN 'pipeline_human_owners','execution_attempt_human_owners' THEN
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',NULL,legacy,true);
 WHEN 'development_workspaces' THEN keys:=ARRAY['created_by'];
 WHEN 'development_workspace_sessions','development_workspace_operations' THEN
  RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>'actor_id');
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',NULL,legacy,true);
  IF source='development_workspace_operations' THEN
   IF document->'recovery_actor_id'<>'null'::jsonb THEN RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>'recovery_actor_id');END IF;
   RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'recovery_owner_subject',NULL,legacy,false);
  END IF;
 WHEN 'pull_request_merge_attempts' THEN
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',NULL,legacy,false);
 WHEN 'outbox_messages' THEN
  IF document->>'channel'='pipeline_trigger' THEN
   RETURN QUERY SELECT * FROM forge_identity_central_ref(document#>'{payload,ownerSubject}',NULL,legacy,false);
  END IF;
 WHEN 'schedules','schedule_fires','repository_sync_connections','repository_sync_runs' THEN
  -- Schedules and syncs store NULL for non-human/native automation.
  -- A present human owner is an exact Auth subject, never a local user alias.
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',NULL,legacy,false);
 ELSE RAISE EXCEPTION 'identity_reference_source_unknown';
 END CASE;
 FOREACH key IN ARRAY COALESCE(keys,ARRAY[]::text[]) LOOP RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>key);END LOOP;
END;
$$;

DO $$ DECLARE source text;
BEGIN
 FOREACH source IN ARRAY ARRAY['development_workspaces','development_workspace_sessions','development_workspace_operations'] LOOP
  EXECUTE format('CREATE TRIGGER aaaa_identity_reference_lock BEFORE INSERT OR UPDATE OR DELETE ON %I FOR EACH STATEMENT EXECUTE FUNCTION forge_identity_write_lock()',source);
  EXECUTE format('CREATE TRIGGER identity_reference_guard BEFORE INSERT OR UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION forge_identity_write_guard()',source);
 END LOOP;
END $$;

ALTER TABLE pipeline_plans DROP CONSTRAINT pipeline_plans_config_source_check;
ALTER TABLE pipeline_plans ADD CONSTRAINT pipeline_plans_config_source_check
 CHECK (config_source IN ('repository','legacy_template','deployment','development_workspace'));
