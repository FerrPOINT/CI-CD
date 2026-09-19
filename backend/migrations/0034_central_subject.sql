ALTER TABLE users ADD COLUMN IF NOT EXISTS central_sub text;
CREATE UNIQUE INDEX IF NOT EXISTS users_central_sub_idx ON users (central_sub) WHERE central_sub IS NOT NULL;
