-- Freshness only. Product admission/role checks remain at their owning boundary.
-- Insert/delete in the same transaction keeps the deferred event without durable rows.
CREATE TABLE forge_request_commit_fences (
    fence_id uuid PRIMARY KEY,
    valid_until timestamptz NOT NULL
);
CREATE FUNCTION forge_request_commit_clock() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF clock_timestamp() >= NEW.valid_until THEN
        RAISE EXCEPTION 'forge_request_authority_expired' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER forge_request_commit_clock
AFTER INSERT ON forge_request_commit_fences
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION forge_request_commit_clock();
