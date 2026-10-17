-- Local SAT rows/FKs remain authoritative and byte-identical. Central children
-- use a disjoint custody variant, never synthetic service_accounts/api_tokens.
ALTER TABLE task_pipeline_intents
    ALTER COLUMN service_account_id DROP NOT NULL,
    ALTER COLUMN api_token_id DROP NOT NULL,
    ADD COLUMN delegated_credential JSONB,
    ADD CONSTRAINT task_pipeline_credential_variant CHECK ((
        (service_account_id IS NOT NULL AND api_token_id IS NOT NULL AND delegated_credential IS NULL)
        OR (service_account_id IS NULL AND api_token_id IS NULL
            AND jsonb_typeof(delegated_credential)='object'
            AND delegated_credential ?& ARRAY['subject','fingerprint','context_hash','context','encrypted_child']
            AND delegated_credential-ARRAY['subject','fingerprint','context_hash','context','encrypted_child']='{}'::jsonb
            AND jsonb_typeof(delegated_credential->'subject')='string'
            AND delegated_credential->>'subject' ~ '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
            AND delegated_credential->>'subject'<>'00000000-0000-0000-0000-000000000000'
            AND jsonb_typeof(delegated_credential->'fingerprint')='string'
            AND delegated_credential->>'fingerprint' ~ '^[0-9a-f]{64}$'
            AND jsonb_typeof(delegated_credential->'context_hash')='string'
            AND delegated_credential->>'context_hash' ~ '^[0-9a-f]{64}$'
            AND jsonb_typeof(delegated_credential->'encrypted_child')='string'
            AND delegated_credential->>'encrypted_child' ~ '^v1:[A-Za-z0-9+/=]+:[A-Za-z0-9+/=]+$'
            AND octet_length(delegated_credential::text)<=65536
            AND jsonb_typeof(delegated_credential->'context')='object'
            AND delegated_credential->'context' ?& ARRAY['version','projectId','intent','runId','chatId','machineSubject','triggerId','variablesSha256']
            AND (delegated_credential->'context')-ARRAY['version','projectId','intent','runId','chatId','machineSubject','triggerId','variablesSha256']='{}'::jsonb
            AND delegated_credential->'context'->'version'='1'::jsonb
            AND delegated_credential->'context'->>'projectId'=project_id::text
            AND delegated_credential->'context'->'intent'=intent
            AND delegated_credential->'context'->>'runId' ~ '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
            AND delegated_credential->'context'->>'runId'<>'00000000-0000-0000-0000-000000000000'
            AND delegated_credential->'context'->>'chatId' ~ '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
            AND delegated_credential->'context'->>'chatId'<>'00000000-0000-0000-0000-000000000000'
            AND jsonb_typeof(delegated_credential->'context'->'machineSubject')='string'
            AND length(delegated_credential->'context'->>'machineSubject') BETWEEN 1 AND 256
            AND delegated_credential->'context'->>'triggerId' ~ '^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$'
            AND delegated_credential->'context'->>'triggerId'<>'00000000-0000-0000-0000-000000000000'
            AND delegated_credential->'context'->>'variablesSha256' ~ '^[0-9a-f]{64}$')
    ) IS TRUE);

ALTER TABLE sdlc_workspace_operations
    ALTER COLUMN service_account_id DROP NOT NULL,
    ADD COLUMN delegated_subject UUID,
    ADD COLUMN delegated_context_hash TEXT,
    ADD CONSTRAINT sdlc_workspace_credential_variant CHECK ((
        (service_account_id IS NOT NULL AND delegated_subject IS NULL AND delegated_context_hash IS NULL)
        OR (service_account_id IS NULL AND delegated_subject IS NOT NULL
            AND delegated_subject<>'00000000-0000-0000-0000-000000000000'::uuid
            AND delegated_context_hash ~ '^[0-9a-f]{64}$')
    ) IS TRUE);

-- Preserve the complete existing attempt/lease/custody guard; change only its
-- original-owner receipt match to recognize the disjoint central variant.
DO $$
DECLARE
    definition text := pg_get_functiondef('guard_task_pipeline_intent()'::regprocedure);
    needle text := 'w.service_account_id=NEW.service_account_id';
BEGIN
    IF length(definition)-length(replace(definition,needle,''))<>length(needle) THEN
        RAISE EXCEPTION 'Expected exactly one task receipt owner predicate';
    END IF;
    EXECUTE replace(definition,needle,
        '(w.service_account_id=NEW.service_account_id OR (NEW.service_account_id IS NULL AND NEW.delegated_credential IS NOT NULL AND w.delegated_subject::text=NEW.delegated_credential->>''subject'' AND w.delegated_context_hash=NEW.delegated_credential->>''context_hash''))');
END;
$$;

CREATE FUNCTION guard_delegated_workspace_receipt() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.delegated_subject IS NOT NULL AND NOT EXISTS(
        SELECT 1 FROM task_pipeline_intents t WHERE t.project_id=NEW.project_id
            AND t.operation_key=NEW.operation_key AND t.service_account_id IS NULL
            AND t.delegated_credential->>'subject'=NEW.delegated_subject::text
            AND t.delegated_credential->>'context_hash'=NEW.delegated_context_hash
            AND t.preparation_request=NEW.receipt->'request') THEN
        RAISE EXCEPTION 'Delegated receipt requires original task credential custody';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER sdlc_workspace_delegated_owner BEFORE INSERT ON sdlc_workspace_operations
    FOR EACH ROW EXECUTE FUNCTION guard_delegated_workspace_receipt();
