# Namespace R: first-publication owner evidence

Source-only additive slice. No runtime, installed capability, migration, issuer or dispatch claim.
The old namespace repository and `/source` response contracts are unchanged.

## Read Contract

`GET /api/v1/namespace-repositories/{repository_id}/devops-evidence`

The slice's router applies the existing `namespace::reader_auth` itself. Only the configured R
machine with central `ci-cd:read` may reach it. Human sessions, unregistered subjects, write-only
tokens, delegated task children and legacy SATs remain denied. This is a server-reader call;
neither the historical Developer child nor a not-yet-issued DevOps child authenticates it.

All query fields are required; unknown or duplicate fields are rejected by the typed extractor:

| Field | Meaning |
| --- | --- |
| `registry_instance_id`, `namespace_id` | Exact current namespace projection |
| `base_ref`, `base_commit` | Exact full branch and current 40-character lowercase owner source SHA |
| `delivery_configuration_name`, `project_id` | Exact same-repository configuration name and UUID |
| `source_pipeline_id`, `operation_key` | Locator of the original retained Developer task pipeline |
| `source_commit` | Exact Developer candidate SHA, not necessarily the project base SHA |
| `request_hash`, `task_id`, `root_task_id`, `assignment_id`, `execution_id`, `fencing_token` | Original `WorkspaceOperationLookup` comparisons |

The namespace/catalog shared advisory lease spans the observation. Project, workspace mapping
and original task intent rows are share-locked. The original receipt is resolved using
`find_owned_receipt`, after rejecting legacy/mixed custody. Its request, hash, source and task
binding must equal the retained preparation and delegated context. Historical encrypted child
bytes are never opened or returned. No child expiry is extended: R is independently authenticated
and only reads historical custody, not that child's authority.

`candidate_evidence_on` performs the existing current-generation, successful pipeline, repository
config/plan and actual artifact-byte checks. The response retains its original blockers and false
dispatch/deployment/acceptance flags. Empty or unobserved artifact evidence is not successful.

## Response

Schema: `forge/namespace-devops-evidence/v1`. Top-level fields:
`schema`, `source`, `candidate`, `target`, `dispatchAllowed:false`. Success is `no-store`.

`source` is the unchanged `VerifiedSource` DTO, including its four-field selected configuration.
`candidate` is the actual `CandidateEvidenceReadback`, not a synthetic `ServerEvidence`.

`target` contains `observedAt`, `targetKind` (`static` or `oci`), `policySha256`,
`taskExecution`, `current`, `confirmed`. `taskExecution` is the existing typed six-field
owner projection: environment, environmentId, repositoryId, trackerInstanceId, trackerProjectId,
holderSubject. **The policy digest is calculated by the existing owner loader over the complete
typed private policy, not over this redacted projection.** Paths, commands, Docker credentials,
probe origins and private manifest/operation contents are not exported.

The existing `delegated::policy` and environment guard require the exact repository/Tracker
project and a same-project, non-protected environment with zero required approvals. Owner marker,
project and full policy hash must match. The route never enrolls a missing target.

Both target kinds use a shared nonblocking read-only flock on the existing `.lock` inode. Missing
lock, busy publication, unknown/torn operation, bad manifest bytes, stale policy or missing pointer
fail closed with a fixed safe code. No filesystem creation or owner publication occurs.

Each pointer is either `{"state":"present","sha256":"..."}` or `{"state":"absent"}`.
Present current must have a retained owner receipt; confirmed must have a Verified owner receipt.
Actual manifest bytes/project/policy are validated through the existing owner reader. Both absent
is permitted only for an enrolled target with no operations and empty manifest/object inventories.
One missing pointer or historical receipts with missing pointers is **not** treated as empty.
These are CAS observations, not a new live health/acceptance assertion.

## Consumer Boundary

Fleet must join this readback to its **actual retained accepted Developer Complete** chain and raw
Tracker receipt hash. A caller's `accepted:true`, PR task link, source SHA or successful CI alone
cannot substitute for that parent chain. This Forge route does not claim Tracker acceptance and
does not accept a caller success boolean. It provides the owner pipeline/operation locator, exact
request hash, artifacts and target policy needed for that server-owned join.

After that join, Fleet can build the existing closed `forgeTaskDelivery` JSON string and commit its
canonical outer variables hash before delegation. Fresh Auth/Tracker/current-child and publication
checks in the existing delivery consumer remain mandatory. An R observation cannot execute a
pipeline/deployment/rollback, renew a lease, mint a child, approve production or override a CAS.

## Integration and Qualification

The root module, protected router and schemas are now registered. The actual Rust-generated
OpenAPI adds only this route and five schemas; all previous paths/schemas are identical and
operation IDs remain unique. Dependencies, source pins and SQL are unchanged.

New source units cover selectors, exact retained custody, legacy/mixed denial, configuration
ownership, candidate truth, missing reader, fixed error redaction, pointers/empty inventories,
target binding and read-lock semantics. The existing isolated central-reader middleware test is
extended to exercise the actual new route for R, foreign, human, child, revoked and SAT inputs.
All fifteen selected Linux cases pass in the existing owned QA, together with fmt, typed compile,
strict Clippy and actual OpenAPI generation. PG-selected0: middleware's missing-DB503 is not a
positive SQL integration claim. Image, installation, live R and Fleet publication remain separate.
[Qualification evidence](evidence/NAMESPACE_DEVOPS_OWNER_20261019.md).
