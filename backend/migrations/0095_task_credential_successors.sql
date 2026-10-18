-- Original task intent, trigger fingerprint and 0092 custody stay immutable.
CREATE TABLE task_credential_successors (
    pipeline_id UUID NOT NULL REFERENCES task_pipeline_intents(pipeline_id) ON DELETE RESTRICT,
    generation BIGINT NOT NULL CHECK (generation > 0),
    renewal_key TEXT NOT NULL CHECK (renewal_key ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    request_hash TEXT NOT NULL CHECK (request_hash ~ '^[0-9a-f]{64}$'),
    predecessor_fingerprint TEXT NOT NULL CHECK (predecessor_fingerprint ~ '^[0-9a-f]{64}$'),
    credential JSONB NOT NULL CHECK ((
        jsonb_typeof(credential)='object'
        AND credential ?& ARRAY['subject','fingerprint','context_hash','context','encrypted_child']
        AND credential-ARRAY['subject','fingerprint','context_hash','context','encrypted_child']='{}'::jsonb
        AND jsonb_typeof(credential->'fingerprint')='string'
        AND credential->>'fingerprint' ~ '^[0-9a-f]{64}$'
        AND jsonb_typeof(credential->'encrypted_child')='string'
        AND credential->>'encrypted_child' ~ '^v1:[A-Za-z0-9+/=]+:[A-Za-z0-9+/=]+$'
        AND octet_length(credential::text)<=65536
        AND credential->>'fingerprint'<>predecessor_fingerprint
    ) IS TRUE),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (pipeline_id,generation),
    UNIQUE (pipeline_id,renewal_key),
    UNIQUE (pipeline_id,predecessor_fingerprint)
);
CREATE UNIQUE INDEX task_credential_successor_fingerprint
    ON task_credential_successors(pipeline_id,(credential->>'fingerprint'));

CREATE FUNCTION guard_task_credential_successor() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    original jsonb;
    previous jsonb;
    previous_generation bigint;
BEGIN
    IF TG_OP<>'INSERT' THEN
        RAISE EXCEPTION 'Task credential successor history is immutable';
    END IF;
    -- Same lock as task preparation/submission: no credential branch or reset.
    SELECT delegated_credential INTO original FROM task_pipeline_intents
        WHERE pipeline_id=NEW.pipeline_id FOR UPDATE;
    IF original IS NULL THEN
        RAISE EXCEPTION 'Credential renewal requires original delegated custody';
    END IF;
    SELECT generation,credential INTO previous_generation,previous
        FROM task_credential_successors WHERE pipeline_id=NEW.pipeline_id
        ORDER BY generation DESC LIMIT 1;
    previous_generation := coalesce(previous_generation,0);
    previous := coalesce(previous,original);
    IF NEW.generation<>previous_generation+1
        OR NEW.predecessor_fingerprint IS DISTINCT FROM previous->>'fingerprint'
        OR (NEW.credential-ARRAY['fingerprint','encrypted_child']) IS DISTINCT FROM
           (original-ARRAY['fingerprint','encrypted_child'])
        OR NEW.credential->>'fingerprint'=original->>'fingerprint'
    THEN
        RAISE EXCEPTION 'Task credential successor changed original context or predecessor';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER task_credential_successor_custody
    BEFORE INSERT OR UPDATE OR DELETE ON task_credential_successors
    FOR EACH ROW EXECUTE FUNCTION guard_task_credential_successor();
