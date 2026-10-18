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

## Exact task-owned PipelineDetail

Task child также может читать существующий `GET /api/v1/pipelines/{pipeline_id}`.
Возвращается прежний `PipelineDetail`, включая actual plan/stages/jobs/status,
без нового DTO. Journal должен принадлежать именно этому child: current successor
fingerprint/subject, context grant, project, task/assignment/run/fence и source
сверяются с первоначальным intent. Текущие Auth expiry/revoke, Tracker lease/native
binding и project/repository mapping проверяются прежним authorizer. Данные
читаются под исходным custody lock; после ожиданий deadline не продлевается.
Успешный child response имеет `Cache-Control: no-store`.

Чужой project/task/pipeline и superseded child отклоняются. List, jobs/logs,
retry/cancel, generic deployment, delivery и rollback routes этим доступом не
открываются. Legacy SAT/User middleware и JSON response shape сохранены.
Queued/failed status не преобразуется в CI PASS. GET task-workspace по-прежнему
возвращает отдельный preparation locator, а не CI result. Установка совместимого
backend и использование actual CI evidence перед ролью Complete остаются
отдельными действиями; blanket запрет Reviewer этим пакетом не вводится.

Fleet renewal journal/private env refresh реализованы отдельным consumer-пакетом
в [Fleet Control #105](https://github.com/FerrPOINT/fleet-control/pull/105).
Его проверка, merge и совместимая установка ещё требуются; наличие исходников
не является live-приёмкой renewal. После exact current ACK Fleet сохраняет свой
receipt, затем атомарно обновляет прежний private env. CLI перечитывает env перед
следующим вызовом. Unknown ACK сначала читается исходным renewal key, не новым
business key.

## Проверка Истории Миграций

Добавлен отдельный ignored PG-сценарий
`task_credential_successor_migration::populated_0094_to_0095_preserves_original_custody_business_and_migration_history`.
Он засеивает SAT/delegated custody, попытки и business queue до применения0095,
затем сравнивает полные исходные строки, прежний ledger/checksums и OID/body
guard functions после upgrade и replay. Successor table остаётся пустой:
сама миграция не выпускает credentials. Source fixture не заменяет live renewal.

CI явно запускает оба ignored renewal-теста и upgrade-сценарий на отдельной БД,
проверяя также число выполненных случаев. Каждый тест создаёт собственную
схему, применяет штатный SQLx catalog и удаляет схему после проверки; заранее
мигрированная public schema не требуется. Для локального запуска задайте
`CICD_TEST_DATABASE_URL` отдельной disposable БД с префиксом `forge_test_`:

```sh
cargo test --locked -p cicd-server --lib renewal_pg_ -- --ignored --test-threads=1
cargo test --locked --features integration --test integration_db task_credential_successor_migration:: -- --ignored --test-threads=1
```

## Квалификация исходников — 18 октября 2026

Исправление task-owned PipelineDetail опубликовано в
[Forge #130](https://github.com/FerrPOINT/CI-CD/pull/130), merge
`49731f66657d277858e90edce1ccac6bb3529cd8`.
[Полный hosted CI](https://github.com/FerrPOINT/CI-CD/actions/runs/39132155553)
прошёл на этом merge. Следующий согласованный источник
`1c2353dc999bad3cf32a003320e8971e8230affb` также имеет
[четыре успешных hosted gates](https://github.com/FerrPOINT/CI-CD/actions/runs/39153643096).

Локально проверены Rust 1.88 fmt, strict workspace/integration clippy, tests и
release build; generated OpenAPI совпал с canonical YAML. HTTP/PG сценарий
renewal/read-detail и populated 0094→0095 запускались явно, с проверкой числа
выполненных тестов. Исторические строки, checksums и guard functions сохранились.
Auth/Fleet ответы в HTTP fixture синтетические: эти source gates не доказывают
настоящий native ACK, обновление private env или завершение agent/CI run.

Эти результаты относятся к исходникам. Наличие миграции и зелёный CI не означают
её применение к установленной БД; совместимая установка Forge/Fleet и live renewal
остаются отдельным этапом. Дополнительный UI pin с тем же backend tree не требует
повторения неизменённых PostgreSQL gates, но квалифицируется своими package/UI
проверками.
