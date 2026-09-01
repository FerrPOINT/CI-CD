-- 0022_runner_work_notifications: wake external runner long-polls across API replicas.

CREATE OR REPLACE FUNCTION notify_runner_work_available()
RETURNS trigger AS $$
DECLARE
    payload text;
BEGIN
    IF NEW.state <> 'queued' THEN
        RETURN NEW;
    END IF;

    IF TG_OP = 'UPDATE'
       AND OLD.state IS NOT DISTINCT FROM NEW.state
       AND OLD.not_before IS NOT DISTINCT FROM NEW.not_before
       AND OLD.required_tags IS NOT DISTINCT FROM NEW.required_tags
    THEN
        RETURN NEW;
    END IF;

    payload := COALESCE(NEW.pipeline_id::text, '');
    PERFORM pg_notify('runner_work_available', payload);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_job_queue_runner_work_available ON job_queue;
CREATE TRIGGER trg_job_queue_runner_work_available
AFTER INSERT OR UPDATE OF state, not_before, required_tags ON job_queue
FOR EACH ROW
EXECUTE FUNCTION notify_runner_work_available();

CREATE OR REPLACE FUNCTION notify_runner_work_unblocked()
RETURNS trigger AS $$
DECLARE
    payload text;
BEGIN
    SELECT s.pipeline_id::text
      INTO payload
      FROM stages s
     WHERE s.id = NEW.stage_id;

    PERFORM pg_notify('runner_work_available', COALESCE(payload, ''));
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_jobs_runner_work_unblocked ON jobs;
CREATE TRIGGER trg_jobs_runner_work_unblocked
AFTER UPDATE OF status, manual ON jobs
FOR EACH ROW
WHEN (
    (OLD.status IS DISTINCT FROM NEW.status AND NEW.status IN ('queued','success','failed','canceled'))
    OR
    (OLD.manual IS DISTINCT FROM NEW.manual AND NEW.status = 'queued')
)
EXECUTE FUNCTION notify_runner_work_unblocked();
