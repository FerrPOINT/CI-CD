-- Identity owner fences cover old SQL ports as well as HTTP/JIT provisioning.
CREATE TABLE identity_legacy_issuer(singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),issuer text NOT NULL CHECK(length(issuer) BETWEEN 1 AND 2048));
CREATE TABLE identity_fence_epoch(singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),revision bigint NOT NULL DEFAULT 1 CHECK(revision>0));
INSERT INTO identity_fence_epoch(singleton) VALUES(true);
CREATE SEQUENCE identity_reference_revision MINVALUE 1;
CREATE TABLE identity_deletion_operations(
 operation_id uuid PRIMARY KEY,issuer text NOT NULL,subject text NOT NULL,inventory_revision uuid NOT NULL,
 request jsonb NOT NULL,state text NOT NULL CHECK(state IN ('frozen','released')),fence uuid,
 revision bigint NOT NULL CHECK(revision>0),reference_count bigint NOT NULL CHECK(reference_count>=0),
 CHECK((state='frozen')=(fence IS NOT NULL)));
CREATE UNIQUE INDEX identity_one_active_freeze ON identity_deletion_operations(issuer,subject) WHERE state='frozen';
-- Exact aliases retain identity meaning after rename, local deletion and central remap.
CREATE TABLE identity_legacy_aliases(
 kind text NOT NULL CHECK(kind IN ('local_user_id','local_principal_id','username','legacy_actor')),
 alias text NOT NULL CHECK(alias<>''),disposition text NOT NULL CHECK(disposition IN ('identity','non_identity')),
 issuer text,subject text,assertion_reference text NOT NULL CHECK(length(assertion_reference) BETWEEN 1 AND 500),
 PRIMARY KEY(kind,alias),
 CHECK((disposition='identity' AND issuer IS NOT NULL AND issuer<>'' AND subject IS NOT NULL AND subject<>'')
    OR (disposition='non_identity' AND issuer IS NULL AND subject IS NULL)));

CREATE FUNCTION forge_identity_assertion_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'identity_mapping_immutable' USING ERRCODE='23514'; END;
$$;
CREATE TRIGGER identity_legacy_issuer_immutable BEFORE UPDATE OR DELETE ON identity_legacy_issuer FOR EACH ROW EXECUTE FUNCTION forge_identity_assertion_immutable();
CREATE TRIGGER identity_legacy_alias_immutable BEFORE UPDATE OR DELETE ON identity_legacy_aliases FOR EACH ROW EXECUTE FUNCTION forge_identity_assertion_immutable();

CREATE FUNCTION forge_identity_write_lock() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 PERFORM pg_advisory_xact_lock_shared(714402216002::bigint);
 PERFORM 1 FROM identity_fence_epoch WHERE singleton FOR SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'identity_fence_epoch_missing' USING ERRCODE='23514'; END IF;
 PERFORM nextval('identity_reference_revision');
 RETURN NULL;
END;
$$;
CREATE FUNCTION forge_identity_alias(kind_value text,alias_value text)
RETURNS TABLE(issuer text,subject text,unresolved boolean) LANGUAGE plpgsql STABLE AS $$
DECLARE mapping record;
BEGIN
 IF alias_value IS NULL THEN RETURN; END IF;
 IF NULLIF(btrim(alias_value),'') IS NULL THEN issuer:=NULL;subject:=NULL;unresolved:=true;RETURN NEXT;RETURN;END IF;
 SELECT * INTO mapping FROM identity_legacy_aliases a WHERE a.kind=kind_value AND a.alias=alias_value;
 IF NOT FOUND THEN issuer:=NULL;subject:=NULL;unresolved:=true;RETURN NEXT;
 ELSIF mapping.disposition='identity' THEN issuer:=mapping.issuer;subject:=mapping.subject;
  unresolved:=NULLIF(btrim(issuer),'') IS NULL OR NULLIF(btrim(subject),'') IS NULL;RETURN NEXT;
 END IF;
END;
$$;
CREATE FUNCTION forge_identity_enroll(kind_value text,alias_value text,issuer_value text,subject_value text)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM identity_legacy_aliases a WHERE a.kind=kind_value AND a.alias=alias_value
     AND a.disposition='identity' AND a.issuer=issuer_value AND a.subject=subject_value) THEN RETURN;END IF;
 INSERT INTO identity_legacy_aliases(kind,alias,disposition,issuer,subject,assertion_reference)
 VALUES(kind_value,alias_value,'identity',issuer_value,subject_value,'current-central-binding') ON CONFLICT DO NOTHING;
 IF NOT EXISTS(SELECT 1 FROM identity_legacy_aliases a WHERE a.kind=kind_value AND a.alias=alias_value
     AND a.disposition='identity' AND a.issuer=issuer_value AND a.subject=subject_value) THEN
  RAISE EXCEPTION 'identity_alias_conflict' USING ERRCODE='23514';
 END IF;
END;
$$;
CREATE FUNCTION forge_identity_user_aliases() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE bound text; central text; document jsonb;
BEGIN
 IF TG_OP='DELETE' THEN document:=to_jsonb(OLD); ELSE document:=to_jsonb(NEW); END IF;
 SELECT l.issuer INTO bound FROM identity_legacy_issuer l WHERE singleton;
 central:=document->>'central_sub';
 IF bound IS NOT NULL AND central IS NOT NULL AND central<>'' THEN
  PERFORM forge_identity_enroll('local_user_id',document->>'id',bound,central);
  PERFORM forge_identity_enroll('local_principal_id',document->>'id',bound,central);
  PERFORM forge_identity_enroll('username',document->>'username',bound,central);
 END IF;
 IF TG_OP='DELETE' THEN RETURN OLD; END IF;
 RETURN NEW;
END;
$$;
-- Runs before row validation; every entry still belongs to the same writer transaction.
CREATE TRIGGER aaaa_identity_user_aliases BEFORE INSERT OR UPDATE OR DELETE ON users FOR EACH ROW EXECUTE FUNCTION forge_identity_user_aliases();

CREATE FUNCTION forge_identity_enroll_machine(id_value text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM identity_legacy_aliases WHERE kind='local_principal_id' AND alias=id_value AND disposition='non_identity') THEN RETURN;END IF;
 INSERT INTO identity_legacy_aliases(kind,alias,disposition,assertion_reference)
 VALUES('local_principal_id',id_value,'non_identity','current-local-service-account') ON CONFLICT DO NOTHING;
 IF NOT EXISTS(SELECT 1 FROM identity_legacy_aliases WHERE kind='local_principal_id' AND alias=id_value AND disposition='non_identity') THEN
  RAISE EXCEPTION 'identity_alias_conflict' USING ERRCODE='23514';END IF;
END;
$$;
CREATE FUNCTION forge_identity_machine_aliases() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP='DELETE' THEN PERFORM forge_identity_enroll_machine(OLD.id::text);RETURN OLD;
 ELSE PERFORM forge_identity_enroll_machine(NEW.id::text);RETURN NEW;END IF;
END;
$$;
CREATE TRIGGER aaaa_identity_machine_aliases BEFORE INSERT OR UPDATE OR DELETE ON service_accounts FOR EACH ROW EXECUTE FUNCTION forge_identity_machine_aliases();

-- Null means legacy/untyped. New supported ports explicitly supply the reference
-- semantics from verified actor/typed UserId parameters; [] means no identity refs.
ALTER TABLE audit_log ADD COLUMN identity_refs jsonb CHECK(identity_refs IS NULL OR jsonb_typeof(identity_refs)='array');
ALTER TABLE releases ADD COLUMN identity_refs jsonb CHECK(identity_refs IS NULL OR jsonb_typeof(identity_refs)='array');
ALTER TABLE deployment_approvals ADD COLUMN identity_refs jsonb CHECK(identity_refs IS NULL OR jsonb_typeof(identity_refs)='array');

CREATE FUNCTION forge_identity_central_ref(subject_value jsonb,issuer_value jsonb,inherited_issuer text,
    required boolean DEFAULT true)
RETURNS TABLE(issuer text,subject text,unresolved boolean) LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
 IF NOT required AND (subject_value IS NULL OR subject_value='null'::jsonb) THEN RETURN;END IF;
 -- Missing issuer may use the explicit legacy assertion; explicit null/malformed issuer cannot.
 IF issuer_value IS NULL THEN issuer:=inherited_issuer;
 ELSIF jsonb_typeof(issuer_value)='string' THEN issuer:=issuer_value#>>'{}';
 ELSE issuer:=NULL;END IF;
 IF jsonb_typeof(subject_value)='string' THEN subject:=subject_value#>>'{}';ELSE subject:=NULL;END IF;
 unresolved:=NULLIF(btrim(issuer),'') IS NULL OR NULLIF(btrim(subject),'') IS NULL;
 RETURN NEXT;
END;
$$;
CREATE FUNCTION forge_identity_typed_refs(document jsonb)
RETURNS TABLE(issuer text,subject text,unresolved boolean) LANGUAGE plpgsql STABLE AS $$
DECLARE entry jsonb; legacy text;
BEGIN
 SELECT l.issuer INTO legacy FROM identity_legacy_issuer l WHERE singleton;
 IF jsonb_typeof(document) IS DISTINCT FROM 'array' THEN
  issuer:=NULL;subject:=NULL;unresolved:=true;RETURN NEXT;RETURN;
 END IF;
 FOR entry IN SELECT value FROM jsonb_array_elements(document) LOOP
  IF jsonb_typeof(entry) IS DISTINCT FROM 'object' THEN
   issuer:=NULL;subject:=NULL;unresolved:=true;RETURN NEXT;
  ELSIF entry->>'kind' IN ('local_user_id','local_principal_id','username','legacy_actor')
     AND jsonb_typeof(entry->'alias')='string' AND entry-ARRAY['kind','alias']='{}'::jsonb THEN
   RETURN QUERY SELECT * FROM forge_identity_alias(entry->>'kind',entry->>'alias');
  ELSIF entry->>'kind'='central' AND entry-ARRAY['kind','subject','issuer']='{}'::jsonb THEN
   RETURN QUERY SELECT * FROM forge_identity_central_ref(entry->'subject',entry->'issuer',legacy,true);
  ELSE issuer:=NULL;subject:=NULL;unresolved:=true;RETURN NEXT;
  END IF;
 END LOOP;
END;
$$;
CREATE FUNCTION forge_identity_row_refs(source text,document jsonb)
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
 WHEN 'schedules','schedule_fires' THEN
  -- The released hybrid scheduler stores NULL for non-human/native automation.
  -- A present human owner is an exact Auth subject, never a local user alias.
  RETURN QUERY SELECT * FROM forge_identity_central_ref(document->'owner_subject',NULL,legacy,false);
 ELSE RAISE EXCEPTION 'identity_reference_source_unknown';
 END CASE;
 FOREACH key IN ARRAY COALESCE(keys,ARRAY[]::text[]) LOOP RETURN QUERY SELECT * FROM forge_identity_alias('local_user_id',document->>key);END LOOP;
END;
$$;
CREATE FUNCTION forge_identity_write_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF EXISTS(SELECT 1 FROM forge_identity_row_refs(TG_TABLE_NAME,to_jsonb(NEW)) r JOIN identity_deletion_operations f
     ON f.issuer=r.issuer AND f.subject=r.subject WHERE f.state='frozen') THEN
  RAISE EXCEPTION 'identity_deletion_fenced' USING ERRCODE='23514';
 END IF;
 IF EXISTS(SELECT 1 FROM identity_deletion_operations WHERE state='frozen')
    AND EXISTS(SELECT 1 FROM forge_identity_row_refs(TG_TABLE_NAME,to_jsonb(NEW)) WHERE unresolved) THEN
  RAISE EXCEPTION 'identity_legacy_reference_unresolved' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END;
$$;
CREATE FUNCTION forge_identity_alias_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.disposition='identity' AND EXISTS(SELECT 1 FROM identity_deletion_operations WHERE issuer=NEW.issuer AND subject=NEW.subject AND state='frozen') THEN
  RAISE EXCEPTION 'identity_deletion_fenced' USING ERRCODE='23514';END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER identity_alias_lock BEFORE INSERT ON identity_legacy_aliases FOR EACH STATEMENT EXECUTE FUNCTION forge_identity_write_lock();
CREATE TRIGGER identity_alias_guard BEFORE INSERT ON identity_legacy_aliases FOR EACH ROW EXECUTE FUNCTION forge_identity_alias_guard();
DO $$
DECLARE source text;
BEGIN
 FOREACH source IN ARRAY ARRAY['users','user_credentials','sessions','api_tokens','project_memberships','tenant_memberships','notification_preferences','forge_confirmed_user_roles',
  'service_accounts','tenants','pull_task_links','repository_attach_operations','delivery_configuration_operations','repository_sync_operations','pull_requests',
  'audit_log','releases','deployment_approvals','forge_space_owner_operations','task_pipeline_intents','task_credential_successors','sdlc_workspace_operations',
  'pipeline_human_owners','execution_attempt_human_owners','schedules','schedule_fires'] LOOP
  EXECUTE format('CREATE TRIGGER aaaa_identity_reference_lock BEFORE INSERT OR UPDATE OR DELETE ON %I FOR EACH STATEMENT EXECUTE FUNCTION forge_identity_write_lock()',source);
  EXECUTE format('CREATE TRIGGER identity_reference_guard BEFORE INSERT OR UPDATE ON %I FOR EACH ROW EXECUTE FUNCTION forge_identity_write_guard()',source);
 END LOOP;
END;
$$;
