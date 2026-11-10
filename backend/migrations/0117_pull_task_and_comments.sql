-- Existing verified links are retained; ambiguous history needs an explicit owner mapping.
ALTER TABLE pull_requests ADD COLUMN primary_tracker_instance_id uuid;
ALTER TABLE pull_requests ADD COLUMN primary_task_id uuid;
ALTER TABLE pull_requests ADD CONSTRAINT pull_primary_task_pair CHECK (
 (primary_tracker_instance_id IS NULL) = (primary_task_id IS NULL)
);
ALTER TABLE pull_requests ADD CONSTRAINT pull_primary_task_link
 FOREIGN KEY (id, primary_tracker_instance_id, primary_task_id)
 REFERENCES pull_task_links(pull_request_id, tracker_instance_id, task_id)
 DEFERRABLE INITIALLY DEFERRED;

ALTER TABLE pull_requests DISABLE TRIGGER identity_reference_guard;
ALTER TABLE pull_requests DISABLE TRIGGER namespace_mutation_guard;
UPDATE pull_requests p SET primary_tracker_instance_id=l.tracker_instance_id,
 primary_task_id=l.task_id
FROM pull_task_links l
WHERE l.pull_request_id=p.id AND
 (SELECT count(*) FROM pull_task_links x WHERE x.pull_request_id=p.id)=1;

SET CONSTRAINTS pull_primary_task_link IMMEDIATE;
ALTER TABLE pull_requests ENABLE TRIGGER identity_reference_guard;
ALTER TABLE pull_requests ENABLE TRIGGER namespace_mutation_guard;

CREATE FUNCTION forge_require_pull_primary_task() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='UPDATE' AND OLD.primary_task_id IS NOT NULL AND
    (OLD.primary_tracker_instance_id,OLD.primary_task_id) IS DISTINCT FROM
    (NEW.primary_tracker_instance_id,NEW.primary_task_id) THEN
   RAISE EXCEPTION 'pull_primary_task_immutable' USING ERRCODE='23514';
 END IF;
 IF (TG_OP='INSERT' OR OLD.primary_task_id IS NOT NULL)
    AND EXISTS(SELECT 1 FROM pull_requests WHERE id=NEW.id AND primary_task_id IS NULL) THEN
   RAISE EXCEPTION 'pull_primary_task_required' USING ERRCODE='23514';
 END IF;
 RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER pull_primary_task_required AFTER INSERT OR UPDATE ON pull_requests
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION forge_require_pull_primary_task();

CREATE TABLE pull_comments (
 id uuid PRIMARY KEY,
 pull_request_id uuid NOT NULL REFERENCES pull_requests(id) ON DELETE RESTRICT,
 author_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
 operation_id uuid NOT NULL,
 base_sha text NOT NULL CHECK(base_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
 head_sha text NOT NULL CHECK(head_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
 severity text NOT NULL CHECK(severity IN ('note','P0','P1','P2','P3')),
 body text NOT NULL CHECK(length(btrim(body))>0),
 anchor jsonb,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(pull_request_id,author_id,operation_id),
 CHECK(anchor IS NULL OR COALESCE((jsonb_typeof(anchor)='object'
   AND jsonb_typeof(anchor->'path')='string'
   AND length(anchor->>'path')>0
   AND anchor->>'side' IN ('old','new')
   AND jsonb_typeof(anchor->'line')='number'
   AND anchor->>'line' ~ '^[1-9][0-9]*$'
   AND anchor-ARRAY['path','side','line']='{}'::jsonb),false))
);
CREATE INDEX pull_comments_page ON pull_comments(pull_request_id,id);

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
 WHEN 'pull_comments' THEN keys:=ARRAY['author_id'];
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
CREATE TRIGGER aaaa_identity_reference_lock BEFORE INSERT OR UPDATE OR DELETE ON pull_comments
 FOR EACH STATEMENT EXECUTE FUNCTION forge_identity_write_lock();
CREATE TRIGGER identity_reference_guard BEFORE INSERT OR UPDATE ON pull_comments
 FOR EACH ROW EXECUTE FUNCTION forge_identity_write_guard();
CREATE FUNCTION forge_pull_comment_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 RAISE EXCEPTION 'pull_comment_immutable' USING ERRCODE='23514';
END $$;
CREATE TRIGGER pull_comment_immutable BEFORE UPDATE OR DELETE ON pull_comments
 FOR EACH ROW EXECUTE FUNCTION forge_pull_comment_immutable();
