CREATE TABLE task_pipeline_intents (
    pipeline_id UUID PRIMARY KEY REFERENCES pipelines(id) ON DELETE RESTRICT,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    service_account_id UUID NOT NULL REFERENCES service_accounts(id) ON DELETE RESTRICT,
    api_token_id UUID NOT NULL REFERENCES api_tokens(id) ON DELETE RESTRICT,
    preparation_job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE RESTRICT,
    operation_key TEXT NOT NULL CHECK (operation_key ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    intent JSONB NOT NULL CHECK (jsonb_typeof(intent)='object' AND octet_length(intent::text)<=8192),
    preparation_request JSONB,
    submitted_attempts UUID[] NOT NULL DEFAULT '{}',
    CHECK (cardinality(submitted_attempts)<=1000 AND COALESCE(array_ndims(submitted_attempts),1)=1
        AND array_position(submitted_attempts,NULL) IS NULL),
    UNIQUE(project_id,operation_key),
    CHECK ((intent ?& ARRAY['operationKey','binding','repositoryId','sourceCommit','role','agentId','trackerLeaseId','holderSubject','expectedLeaseVersion']
        AND intent-ARRAY['operationKey','binding','repositoryId','sourceCommit','role','agentId','trackerLeaseId','holderSubject','expectedLeaseVersion']='{}'::jsonb
        AND jsonb_typeof(intent->'operationKey')='string' AND intent->>'operationKey'=operation_key
        AND jsonb_typeof(intent->'binding')='object'
        AND jsonb_typeof(intent->'sourceCommit')='string' AND intent->>'sourceCommit' ~ '^[0-9a-f]{40}$'
        AND jsonb_typeof(intent->'role')='string' AND intent->>'role' IN ('analyst','architect','developer','reviewer','tester','devops')
        AND jsonb_typeof(intent->'holderSubject')='string' AND length(intent->>'holderSubject') BETWEEN 1 AND 256
        AND jsonb_typeof(intent->'expectedLeaseVersion')='number'
        AND intent->>'expectedLeaseVersion' ~ '^[1-9][0-9]{0,15}$'
        AND (intent->>'expectedLeaseVersion')::bigint BETWEEN 1 AND 9007199254740991
        AND jsonb_typeof(intent->'repositoryId')='string' AND (intent->>'repositoryId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'agentId')='string' AND (intent->>'agentId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'trackerLeaseId')='string' AND (intent->>'trackerLeaseId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid) IS TRUE),
    CHECK ((intent->'binding' ?& ARRAY['trackerInstanceId','trackerProjectId','taskId','rootTaskId','assignmentId','executionId','routingSnapshotId','requirementRevision','fencingToken','assignmentHash','workflowTaskRef']
        AND (intent->'binding')-ARRAY['trackerInstanceId','trackerProjectId','taskId','rootTaskId','assignmentId','executionId','routingSnapshotId','requirementRevision','fencingToken','assignmentHash','workflowTaskRef']='{}'::jsonb
        AND jsonb_typeof(intent->'binding'->'trackerInstanceId')='string'
        AND (intent->'binding'->>'trackerInstanceId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND (intent->'binding'->>'trackerInstanceId')::uuid::text=intent->'binding'->>'trackerInstanceId'
        AND jsonb_typeof(intent->'binding'->'trackerProjectId')='string'
        AND (intent->'binding'->>'trackerProjectId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'binding'->'taskId')='string'
        AND (intent->'binding'->>'taskId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'binding'->'rootTaskId')='string'
        AND (intent->'binding'->>'rootTaskId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'binding'->'assignmentId')='string'
        AND (intent->'binding'->>'assignmentId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'binding'->'executionId')='string'
        AND (intent->'binding'->>'executionId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(intent->'binding'->'routingSnapshotId')='string'
        AND (intent->'binding'->>'routingSnapshotId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND intent->'binding'->>'taskId'=intent->'binding'->>'rootTaskId'
        AND jsonb_typeof(intent->'binding'->'assignmentHash')='string'
        AND intent->'binding'->>'assignmentHash' ~ '^[0-9a-f]{64}$'
        AND jsonb_typeof(intent->'binding'->'workflowTaskRef')='string'
        AND intent->'binding'->>'workflowTaskRef' ~ '^SDLC-[1-9][0-9]{0,18}$'
        AND jsonb_typeof(intent->'binding'->'requirementRevision')='number'
        AND (intent->'binding'->>'requirementRevision')::bigint BETWEEN 1 AND 9007199254740991
        AND jsonb_typeof(intent->'binding'->'fencingToken')='number'
        AND (intent->'binding'->>'fencingToken')::bigint BETWEEN 1 AND 9007199254740991) IS TRUE),
    CHECK (preparation_request IS NULL OR (jsonb_typeof(preparation_request)='object'
        AND octet_length(preparation_request::text)<=8192
        AND preparation_request ?& ARRAY['contractVersion','operationKey','binding','repositoryId','sourceCommit','leaseId','attemptId','workspaceGeneration','workspaceId','role','access']
        AND preparation_request-ARRAY['contractVersion','operationKey','binding','repositoryId','sourceCommit','leaseId','attemptId','workspaceGeneration','workspaceId','role','access']='{}'::jsonb
        AND preparation_request->'binding'=intent->'binding'
        AND jsonb_typeof(preparation_request->'contractVersion')='number'
        AND preparation_request->>'contractVersion'='1'
        AND preparation_request->>'operationKey'=operation_key
        AND preparation_request->'repositoryId'=intent->'repositoryId'
        AND preparation_request->'sourceCommit'=intent->'sourceCommit'
        AND preparation_request->'role'=intent->'role'
        AND jsonb_typeof(preparation_request->'access')='string'
        AND preparation_request->>'access'='read_only'
        AND jsonb_typeof(preparation_request->'leaseId')='string'
        AND (preparation_request->>'leaseId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(preparation_request->'attemptId')='string'
        AND (preparation_request->>'attemptId')::uuid<>'00000000-0000-0000-0000-000000000000'::uuid
        AND jsonb_typeof(preparation_request->'workspaceGeneration')='number'
        AND preparation_request->>'workspaceGeneration' ~ '^[1-9][0-9]{0,18}$'
        AND (preparation_request->>'workspaceGeneration')::bigint>0
        AND jsonb_typeof(preparation_request->'workspaceId')='string'
        AND preparation_request ?& ARRAY['leaseId','attemptId','workspaceGeneration','workspaceId']) IS TRUE)
);

CREATE FUNCTION guard_task_pipeline_intent() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'Task pipeline intent is immutable';
    END IF;
    IF TG_OP='INSERT' THEN
        IF NEW.preparation_request IS NOT NULL OR cardinality(NEW.submitted_attempts)<>0 OR NOT EXISTS(
            SELECT 1 FROM jobs j JOIN stages s ON s.id=j.stage_id JOIN pipelines p ON p.id=s.pipeline_id
            WHERE j.id=NEW.preparation_job_id AND p.id=NEW.pipeline_id AND p.project_id=NEW.project_id
                AND p.commit_sha=NEW.intent->>'sourceCommit') THEN
            RAISE EXCEPTION 'Task pipeline source identity mismatch';
        END IF;
    ELSE
        IF (to_jsonb(NEW)-ARRAY['preparation_request','submitted_attempts']) IS DISTINCT FROM
            (to_jsonb(OLD)-ARRAY['preparation_request','submitted_attempts']) THEN
            RAISE EXCEPTION 'Task intent identity is immutable';
        END IF;
        IF OLD.preparation_request IS NULL AND NEW.preparation_request IS NOT NULL
            AND OLD.submitted_attempts=NEW.submitted_attempts THEN
          IF NOT EXISTS(SELECT 1 FROM job_leases l JOIN execution_attempts a ON a.id=l.attempt_id AND a.job_id=l.job_id
            WHERE l.job_id=NEW.preparation_job_id AND l.id=(NEW.preparation_request->>'leaseId')::uuid
                AND a.id=(NEW.preparation_request->>'attemptId')::uuid
                AND l.generation=(NEW.preparation_request->>'workspaceGeneration')::bigint
                AND l.runner_id IS NULL AND l.runner_name='embedded' AND l.lease_status='active'
                AND l.lease_expires_at>clock_timestamp() AND l.cancel_requested_at IS NULL
                AND a.status='running'
                AND jsonb_typeof(NEW.preparation_request->'workspaceId')='string'
                AND NEW.preparation_request->>'workspaceId' ~ ('^attempt-'||a.id::text||'-'||l.generation::text||'-[0-9a-f]{32}$')
                AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=l.job_id)) THEN
              RAISE EXCEPTION 'Task preparation requires the actual current embedded lease';
          END IF;
        ELSIF NEW.preparation_request IS NOT DISTINCT FROM OLD.preparation_request
            AND cardinality(NEW.submitted_attempts)=cardinality(OLD.submitted_attempts)+1
            AND NEW.submitted_attempts[1:cardinality(OLD.submitted_attempts)]=OLD.submitted_attempts
            AND NOT(NEW.submitted_attempts[cardinality(NEW.submitted_attempts)]=ANY(OLD.submitted_attempts)) THEN
          IF NOT EXISTS(SELECT 1 FROM execution_attempts a JOIN jobs j ON j.id=a.job_id
              JOIN stages s ON s.id=j.stage_id JOIN job_leases l ON l.attempt_id=a.id AND l.job_id=j.id
              WHERE a.id=NEW.submitted_attempts[cardinality(NEW.submitted_attempts)]
                AND s.pipeline_id=NEW.pipeline_id AND a.status='running' AND j.status='running'
                AND l.runner_id IS NULL AND l.runner_name='embedded' AND l.lease_status='active'
                AND l.lease_expires_at>clock_timestamp() AND l.cancel_requested_at IS NULL
                AND l.generation=(SELECT max(generation) FROM job_leases WHERE job_id=j.id)
                AND (j.id<>NEW.preparation_job_id OR EXISTS(
                    SELECT 1 FROM sdlc_workspace_operations w WHERE w.project_id=NEW.project_id
                    AND w.service_account_id=NEW.service_account_id AND w.operation_key=NEW.operation_key
                    AND w.receipt->'request'=NEW.preparation_request
                    AND (NEW.preparation_request->>'attemptId')::uuid=a.id))) THEN
              RAISE EXCEPTION 'Task submission requires the current original embedded attempt';
          END IF;
        ELSE
            RAISE EXCEPTION 'Task custody cannot be reset or replaced';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER task_pipeline_intent_custody BEFORE INSERT OR UPDATE OR DELETE
    ON task_pipeline_intents FOR EACH ROW EXECUTE FUNCTION guard_task_pipeline_intent();
