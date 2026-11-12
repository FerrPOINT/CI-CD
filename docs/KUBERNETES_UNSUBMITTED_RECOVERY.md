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
resubmit START.

When Kubernetes returns a Forbidden Status for the exact Pod CREATE, Forge
records the request as issued and stores a separate `not-admitted.json`
receipt. The receipt binds the 403 result to the original scope and intent.
Forge checks namespace, PVC and Pod absence before reporting failure. It
retains the original receipt through cleanup without a Pod UID or an exit code.

For an older issued request whose reply was lost, the installation owner may
project `unadmitted.json` into the existing read-only native closure mount
of the recovery runner. The `forge/native-unadmitted-attempt/v1` observation
binds the original scope, Pod name, intent SHA-256, issued timestamp and the
installation, CA, namespace and PVC identities. It carries the observation
timestamp and SHA-256 of the storage history evidence. The owner must verify
complete, overlapping history covering the request and an absent target key.
Keep the source observations with the operation receipt. Only the original
request may seal this evidence under its submission lock. A stale observation
or a changed intent leaves it held.

After lease expiry, the server checks the retained non-admission receipt and
actual Pod absence before acknowledging the original failed or canceled lease.
If neither an explicit rejection nor qualified owner evidence is available,
the issued request requires exact Pod, termination and acknowledgment evidence.

Focused tests cover expired admission before issue, absent preparation intent,
foreign Pod, issued request, submission lock, receipt identity and false server
ACK preservation. Existing shell result/unknown-process and issued terminal
tests remain the neighboring qualification boundary.
