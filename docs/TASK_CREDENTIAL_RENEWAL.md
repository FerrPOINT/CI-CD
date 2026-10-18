# Обновление task credential без повторного pipeline

Расширение existing Forge custody. HTTP routes, authorization policy, OpenAPI
и generated client зарегистрированы в этом комплекте. Установка и совместная
live-приёмка с Fleet остаются отдельным этапом; наличие исходников их не доказывает.

## Authority и wire

Server producer вызывает прежний Base `POST /auth/tokens/delegate` с новым
credential-only key. Business key, trigger UUID, pipeline, intent, context,
variables/source hashes и native run/chat не меняются. Parent P не выдаётся agent.

POST `/api/v1/projects/{project_id}/pipelines/{pipeline_id}/credential-renewals`
принимает strict camelCase JSON: `renewalKey`, `predecessorFingerprint`,
`credentialContext` (полный прежний TaskCredentialContext), `replacementToken`.
Raw token не сериализуется в response, receipt или Debug.

Bearer авторизует exact configured `delegated_subject` P, не local SAT subject.
Только platform PAT без role/session, с ровно `ci-cd:read`, `ci-cd:write`;
child grant как owner запрещён. Нет human/admin/trusted-network fallback.
Root authentication повторяется после DB/network ожиданий. Expired/revoked
predecessor является только данными, никогда auth credential этой операции.

Replacement обязан иметь того же subject и ровно прежние три scopes, включая
canonical context grant. Existing issuer introspection с постоянным
`include_lifetime=true` проверяет identity и clocks в одном ответе. Отсутствие
clocks закрывает эффект. Permit ограничен minimum issuer и прежнего Tracker
deadline; Tracker30s не меняется. Проверяются existing assignment/fence/lease,
routing, native tuple и project/repository mapping.

## Custody и unknown ACK

0095 добавляет только `task_credential_successors`: append-only generation,
renewal key/request hash, predecessor fingerprint и private encrypted credential.
Original intent row lock сериализует CAS и current reads. SQL запрещает
ветвление, update/delete, смену context/subject и возврат original fingerprint.
Исторические 0092/0094, original child и trigger fingerprint остаются прежними.
Renewal не вызывает trigger, queue, prepare, spawn или dispatch.

Повтор key/body возвращает прежний ACK; другой payload или stale predecessor
даёт409. GET того же пути с `/{renewal_key}` выполняет owner auth и current
Tracker readback, возвращает ACK без повторной issuance/effect. Response:
pipelineId, renewalKey, requestHash, generation, predecessorFingerprint,
fingerprint, contextHash, recordedAt, current. Secrets/ciphertext не возвращаются;
ответы `Cache-Control: no-store`. Historical ACK с `current=false` нельзя
использовать для публикации superseded child.

Normal prepare/spawn/publication/readback используют acknowledged-current
overlay с прежними live introspection/revoke/fence guards. SAT path не меняется.
Повторный pipeline POST новым child не является renewal: original trigger
fingerprint намеренно не переписывается.

Fleet renewal journal/private env refresh остаются отдельным consumer-пакетом.
После exact current ACK Fleet должен сохранить свой receipt, затем атомарно
обновить прежний private env. CLI перечитывает env перед следующим вызовом.
Unknown ACK сначала читается исходным renewal key, не новым business key.
