-- Fixed freshness for ordinary SQLx writes, independent of human permissions.
-- A connection checkout stamps the original proof; technical checkouts clear it.
CREATE FUNCTION forge_human_request_commit_clock() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_until text;
BEGIN
    original_until := current_setting('forge.request_valid_until', true);
    IF original_until IS NOT NULL AND original_until <> ''
       AND clock_timestamp() >= original_until::timestamptz THEN
        RAISE EXCEPTION 'forge_request_authority_expired' USING ERRCODE='42501';
    END IF;
    RETURN NULL;
END $$;

-- Future migrations adding writable tables must call this idempotent installer.
-- The native transport regression checks coverage of every public product table.
CREATE FUNCTION forge_install_human_request_clocks() RETURNS void LANGUAGE plpgsql AS $$
DECLARE target record;
BEGIN
    FOR target IN
        SELECT c.oid, n.nspname, c.relname FROM pg_class c
        JOIN pg_namespace n ON n.oid=c.relnamespace
        WHERE n.nspname='public' AND c.relkind='r' AND c.relname <> '_sqlx_migrations'
    LOOP
        IF NOT EXISTS (SELECT 1 FROM pg_trigger
                       WHERE tgrelid=target.oid AND tgname='forge_human_request_clock') THEN
            EXECUTE format(
                'CREATE CONSTRAINT TRIGGER forge_human_request_clock AFTER INSERT OR UPDATE OR DELETE ON %I.%I DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION forge_human_request_commit_clock()',
                target.nspname,target.relname);
        END IF;
    END LOOP;
END $$;
SELECT forge_install_human_request_clocks();
