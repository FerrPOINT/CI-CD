ALTER TABLE artifacts ADD COLUMN IF NOT EXISTS sha256 TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'artifacts_sha256_format'
          AND conrelid = 'artifacts'::regclass
    ) THEN
        ALTER TABLE artifacts
            ADD CONSTRAINT artifacts_sha256_format
            CHECK (sha256 IS NULL OR sha256 ~ '^[0-9a-f]{64}$');
    END IF;
END $$;
