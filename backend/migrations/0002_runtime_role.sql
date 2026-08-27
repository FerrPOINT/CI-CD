-- 0002_runtime_role: least-privilege runtime grants (MIGRATION_CONTRACT §roles).
-- forge_owner owns schema objects and runs cicd-migrate; forge_runtime is the
-- API/worker role and gets only DML on application tables.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'forge_runtime') THEN
        CREATE ROLE forge_runtime NOLOGIN;
    END IF;
END
$$;

GRANT USAGE ON SCHEMA public TO forge_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO forge_runtime;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO forge_runtime;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO forge_runtime;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT ON SEQUENCES TO forge_runtime;
