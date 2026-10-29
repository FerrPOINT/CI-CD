-- A saved automation owner is attribution, never cached authorization.
-- Legacy connections/events remain unbound; do not infer an owner from local IDs.
ALTER TABLE repository_sync_connections ADD COLUMN owner_subject uuid
    CHECK(owner_subject <> '00000000-0000-0000-0000-000000000000');
ALTER TABLE repository_sync_runs ADD COLUMN owner_subject uuid
    CHECK(owner_subject <> '00000000-0000-0000-0000-000000000000');

-- Extend the released identity inventory without changing its existing cases.
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
 WHEN 'schedules','schedule_fires','repository_sync_connections','repository_sync_runs' THEN
  -- Schedules and syncs store NULL for non-human/native automation.
  -- A present human owner is an exact Auth subject, never a local user alias.
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',NULL,legacy,false);
 ELSE RAISE EXCEPTION 'identity_reference_source_unknown';
 END CASE;
 FOREACH key IN ARRAY COALESCE(keys,ARRAY[]::text[]) LOOP RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>key);END LOOP;
END;
$$;
DO $$
DECLARE source text;
BEGIN
 FOREACH source IN ARRAY ARRAY['repository_sync_connections','repository_sync_runs'] LOOP
  EXECUTE format('CREATE TRIGGER aaaa_identity_reference_lock BEFORE INSERT OR UPDATE OR DELETE ON %I FOR EACH STATEMENT EXECUTE FUNCTION forge_identity_write_lock()',source);
  EXECUTE format('CREATE TRIGGER identity_reference_guard BEFORE INSERT OR UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION forge_identity_write_guard()',source);
 END LOOP;
END;
$$;
