-- Declared task bindings are not authoritative admission or workspace receipts.
CREATE TABLE sdlc_workspace_operations (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    service_account_id UUID NOT NULL REFERENCES service_accounts(id) ON DELETE RESTRICT,
    operation_key TEXT NOT NULL CHECK (operation_key ~ '^[A-Za-z0-9_.:-]{1,128}$'),
    request_hash TEXT NOT NULL CHECK (request_hash ~ '^[0-9a-f]{64}$'),
    task_id UUID NOT NULL,
    root_task_id UUID NOT NULL,
    assignment_id UUID NOT NULL,
    execution_id UUID NOT NULL,
    fencing_token BIGINT NOT NULL CHECK (fencing_token > 0),
    lease_id UUID NOT NULL REFERENCES job_leases(id) ON DELETE RESTRICT,
    attempt_id UUID NOT NULL REFERENCES execution_attempts(id) ON DELETE RESTRICT,
    workspace_generation BIGINT NOT NULL CHECK (workspace_generation > 0),
    repository_id UUID NOT NULL REFERENCES repositories(id) ON DELETE RESTRICT,
    source_commit TEXT NOT NULL CHECK (source_commit ~ '^[0-9a-f]{40}$'),
    receipt JSONB NOT NULL CHECK (jsonb_typeof(receipt) = 'object'),
    recorded_at TIMESTAMPTZ NOT NULL,
    UNIQUE (project_id, operation_key),
    CHECK (receipt->>'schema' = 'forge/workspace-operation-receipt/v1'),
    CHECK (receipt->>'status' = 'blocked' AND receipt->>'dispatchAllowed' = 'false'),
    CHECK (receipt->>'operationId' = id::text AND receipt->>'projectId' = project_id::text),
    CHECK (receipt->>'requestHash' = request_hash AND receipt->'request'->>'operationKey' = operation_key),
    CHECK (receipt->'request'->'binding'->>'taskId' = task_id::text
        AND receipt->'request'->'binding'->>'rootTaskId' = root_task_id::text
        AND receipt->'request'->'binding'->>'assignmentId' = assignment_id::text
        AND receipt->'request'->'binding'->>'executionId' = execution_id::text
        AND (receipt->'request'->'binding'->>'fencingToken')::bigint = fencing_token),
    CHECK (receipt->'request'->>'leaseId' = lease_id::text
        AND receipt->'request'->>'attemptId' = attempt_id::text
        AND (receipt->'request'->>'workspaceGeneration')::bigint = workspace_generation
        AND receipt->'request'->>'repositoryId' = repository_id::text
        AND receipt->'request'->>'sourceCommit' = source_commit),
    -- JSONB containment is false for absent fields, unlike nullable CHECK equality.
    CHECK (receipt @> jsonb_build_object(
        'schema','forge/workspace-operation-receipt/v1','status','blocked','dispatchAllowed',false,
        'operationId',id::text,'projectId',project_id::text,'requestHash',request_hash,
        'blockers',jsonb_build_array('tracker_admission_unavailable','tracker_workspace_binding_unavailable'),
        'request',jsonb_build_object('contractVersion',1,'operationKey',operation_key,
            'repositoryId',repository_id::text,'sourceCommit',source_commit,'leaseId',lease_id::text,
            'attemptId',attempt_id::text,'workspaceGeneration',workspace_generation,
            'binding',jsonb_build_object('taskId',task_id::text,'rootTaskId',root_task_id::text,
                'assignmentId',assignment_id::text,'executionId',execution_id::text,'fencingToken',fencing_token)))),
    CHECK (task_id <> '00000000-0000-0000-0000-000000000000'::uuid
        AND root_task_id <> '00000000-0000-0000-0000-000000000000'::uuid
        AND assignment_id <> '00000000-0000-0000-0000-000000000000'::uuid
        AND execution_id <> '00000000-0000-0000-0000-000000000000'::uuid)
);
CREATE INDEX sdlc_workspace_operations_binding
    ON sdlc_workspace_operations (project_id, task_id, assignment_id, execution_id, fencing_token);
CREATE INDEX sdlc_workspace_operations_lease ON sdlc_workspace_operations (lease_id);

CREATE FUNCTION reject_sdlc_workspace_operation_mutation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'SDLC workspace operation receipts are immutable';
END;
$$;
CREATE TRIGGER sdlc_workspace_operations_immutable BEFORE UPDATE OR DELETE
    ON sdlc_workspace_operations FOR EACH ROW EXECUTE FUNCTION reject_sdlc_workspace_operation_mutation();
