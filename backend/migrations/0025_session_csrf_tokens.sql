-- 0025_session_csrf_tokens: CSRF proof for cookie-backed refresh sessions.

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS csrf_token_hash TEXT;
