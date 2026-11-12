-- Existing assignments remain intact. No unknown privilege is confirmed by copying it.
CREATE TABLE forge_confirmed_user_roles(
 user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 role text NOT NULL CHECK(role IN ('maintainer','admin')),
 provenance text NOT NULL CHECK(length(btrim(provenance)) BETWEEN 1 AND 1024),
 confirmed_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE FUNCTION forget_forge_role_confirmation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.role IS DISTINCT FROM OLD.role OR NEW.central_sub IS DISTINCT FROM OLD.central_sub THEN
  DELETE FROM forge_confirmed_user_roles WHERE user_id=OLD.id;
 END IF;
 RETURN NEW;
END;
$$;
CREATE TRIGGER forget_forge_role_confirmation AFTER UPDATE OF role,central_sub ON users
 FOR EACH ROW EXECUTE FUNCTION forget_forge_role_confirmation();
