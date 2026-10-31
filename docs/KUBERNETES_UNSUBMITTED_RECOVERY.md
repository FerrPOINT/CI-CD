# Kubernetes execution not submitted

The external runner records the request as issued before sending Pod CREATE.
A failure before that write is not an unknown submitted execution. It has no
Pod UID or process exit code. Docker and shell execution retain their existing
completion and recovery contracts.

Before closing an unsubmitted request, Forge serializes against preparation and
submission, verifies the actual namespace/PVC identities and Bound state, and
reads the deterministic Pod name as absent. It retains a distinct
not-submitted.json; future preparation/START of that attempt is refused.
A workspace that failed before an execution intent exists uses the same checks
and a separate forge/kubernetes-unprepared/v1 receipt.

For a current lease, normal authenticated failed completion and independent
lease receipt readback authorize local workspace cleanup. On recovery, a real
closed failed/canceled receipt for the exact runner lease/attempt/generation
also authorizes cleanup of NEVER-SUBMITTED work. Its actual
terminalAcknowledged flag is retained unchanged in
not-submitted-owner-readback.json. A user cancellation is not described as a
server completion ACK or native process termination.

Use forge-runner --inspect-workspaces --work-dir /workspaces, then
--reconcile-workspaces with the original controller configuration and runner
credential in its environment. An active lease is not an expired cleanup grant.
Do not run a second polling owner, remove journals, invent terminal records or
resubmit START. The issued-request path continues to require the existing exact
Pod/terminal/ACK/cleanup evidence; Pod absence alone never authorizes it.

Focused tests cover expired admission before issue, absent preparation intent,
foreign Pod, issued request, submission lock, receipt identity and false server
ACK preservation. Existing shell result/unknown-process and issued terminal
tests remain the neighboring qualification boundary.
