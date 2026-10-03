ALTER TABLE deployments ADD COLUMN request_key uuid;

CREATE UNIQUE INDEX deployments_environment_request_key_idx
    ON deployments (environment_id, request_key);
