-- Optional deployment adapter metadata; old environments keep their behavior.
ALTER TABLE environments ADD COLUMN kubernetes_target JSONB;
ALTER TABLE environments ADD CONSTRAINT environments_kubernetes_target_object
    CHECK (kubernetes_target IS NULL OR jsonb_typeof(kubernetes_target) = 'object');
