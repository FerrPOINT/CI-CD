-- 0029_service_accounts: machine principals for automation (AUTHORIZATION
-- target step: service-account tokens without the tenant model yet).
-- api_tokens gains principal_type/service_account_id per the target schema;
-- existing user tokens backfill as principal_type='user'.

CREATE TABLE IF NOT EXISTS service_accounts (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE api_tokens
    ADD COLUMN IF NOT EXISTS principal_type TEXT NOT NULL DEFAULT 'user'
        CHECK (principal_type IN ('user', 'service_account')),
    ADD COLUMN IF NOT EXISTS service_account_id UUID
        REFERENCES service_accounts(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_service_accounts_enabled
    ON service_accounts(enabled);
CREATE INDEX IF NOT EXISTS idx_api_tokens_service_account
    ON api_tokens(service_account_id) WHERE service_account_id IS NOT NULL;
