-- A technical insertion sequence orders new CI deployments, including equal timestamps.
ALTER TABLE pipelines ADD COLUMN delivery_order BIGINT GENERATED ALWAYS AS IDENTITY;
CREATE UNIQUE INDEX pipelines_delivery_order_unique ON pipelines(delivery_order);
-- Runtime records extend the existing deployment history; no legacy row is rewritten.
ALTER TABLE deployments ADD COLUMN execution_attempt_id UUID REFERENCES execution_attempts(id) ON DELETE SET NULL;
ALTER TABLE deployments ADD COLUMN runtime_phase TEXT
    CHECK (runtime_phase IS NULL OR runtime_phase IN ('prepared','running','held','finished'));
ALTER TABLE deployments ADD COLUMN runtime_input JSONB;
ALTER TABLE deployments ADD COLUMN runtime_receipt JSONB;
CREATE UNIQUE INDEX deployments_runtime_attempt_unique
    ON deployments(execution_attempt_id) WHERE execution_attempt_id IS NOT NULL;
CREATE UNIQUE INDEX deployments_runtime_environment_active
    ON deployments(environment_id)
    WHERE runtime_phase IN ('prepared','running','held');
