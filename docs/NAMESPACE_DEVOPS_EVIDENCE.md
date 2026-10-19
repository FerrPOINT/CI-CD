# Namespace R: наблюдение DevOps candidate и owner target

Исходниковый additive-срез. Старые namespace repository и `/source` контракты
сохранены. Наличие route/миграций не означает установленную capability или
полномочия на dispatch. Qualification текущего среза описана отдельно ниже.

## Контракт чтения

`GET /api/v1/namespace-repositories/{repository_id}/devops-evidence`

Router сам применяет существующий `namespace::reader_auth`: только настроенная
registered-R machine с central `ci-cd:read`. Human session, чужой subject,
write-only token, delegated task child и legacy SAT не получают доступ.
Исторический Developer child и будущий DevOps child не авторизуют этот GET.

Все query-поля обязательны; неизвестные и повторные поля отклоняет typed extractor.

| Поле | Назначение |
| --- | --- |
| `registry_instance_id`, `namespace_id` | Точная текущая namespace projection |
| `base_ref`, `base_commit` | Полный branch ref и текущий lowercase owner source SHA из 40 символов |
| `delivery_configuration_name`, `project_id` | Имя и UUID конфигурации того же repository |
| `source_pipeline_id`, `operation_key` | Locator исходного retained Developer task pipeline |
| `source_commit` | Точный Developer candidate SHA, не обязательно project base SHA |
| `request_hash`, `task_id`, `root_task_id`, `assignment_id`, `execution_id`, `fencing_token` | Исходные сравнения `WorkspaceOperationLookup` |

Catalog/namespace advisory lease покрывает наблюдение. Project, workspace mapping
и original task intent читаются под share locks. После запрета legacy/mixed custody
`find_owned_receipt` находит настоящий receipt. Request/hash/source/task binding
должны совпасть с retained preparation и delegated context. Зашифрованный child
не открывается, не возвращается и не продлевается: R читает историческую custody,
а не заимствует полномочия того child.

`candidate_evidence_on` проверяет current generation, successful pipeline,
repository config/plan, latest job attempts, terminal ACK и фактические artifact
bytes. Пустой или неподтверждённый inventory не считается успешным.

## Ответ

Schema `forge/namespace-devops-evidence/v1`: `schema`, `source`, `candidate`,
`target`, `dispatchAllowed:false`; успешный ответ имеет `Cache-Control: no-store`.

`source` — прежний `VerifiedSource` с выбранной four-field delivery configuration.
`candidate` — настоящий `CandidateEvidenceReadback`, не synthetic ServerEvidence.
Его blockers и false dispatch/deployment/acceptance flags сохраняются.

`target` содержит `observedAt`, `targetKind` (`static`/`oci`), `policySha256`,
`taskExecution`, `current`, `confirmed` и optional `selection`.
`taskExecution` содержит только environment, environmentId, repositoryId,
trackerInstanceId, trackerProjectId, holderSubject. Policy hash вычисляется по
полной typed private policy, не по redacted projection. Private paths, commands,
probe origins, Docker credentials и содержимое owner journals не экспортируются.

### Явный owner selection

Приватная owner policy может задать `deliverySelection` с version1, action `deploy`
и точными `stageName`, `jobName`, `artifactName`. Для OCI поле находится в `checks`.
Это защищённая owner-конфигурация, не query/body calling agent. Без поля policy
сохраняет прежние serialized bytes/hash, ответ не содержит `selection`.

Если policy задана, `target.selection` имеет schema
`forge/owner-delivery-selection/v1`, action `deploy`, sourcePipelineId,
configSha256, planSha256 и полный CandidateArtifactEvidence. Resolver требует
ровно один current retained artifact исходного pipeline с указанными именами,
однозначную declared path mapping и точное membership в проверенном candidate.
Missing/duplicate/stale/foreign/ambiguous evidence отклоняется, `first()` и
basename/extension guessing отсутствуют. Автоматического выбора rollback нет.
[Полный контракт selection](DEVOPS_OWNER_SELECTION.md).

Оба runner и observer используют общее кодирование artifact paths: `/` и `\`
становятся `__`, без другой нормализации. Stored name `dist__bundle.tar` не равен
literal path `dist/bundle.tar` или basename `bundle.tar`. Повторные declarations,
encoded aliases и duplicate observed names запрещены; старый attempt не заменяет
отсутствующий output текущего. Существующие filesystem guards не ослаблены;
HTTP upload protocol и исторические artifact rows не переписываются.

## Target custody и CAS

Существующие owner loader и environment guard требуют тот же repository/Tracker
project и same-project nonprotected environment без required approvals.
Owner marker, project и полный policy hash должны совпасть. GET не enroll-ит target.

Static/OCI используют nonblocking shared flock существующего `.lock` inode.
Missing lock, busy publication, unknown/torn operation, неверные manifest bytes,
stale policy или недоказанные pointers закрывают readback с фиксированным safe code.
Ни файлов, ни publication операций этот путь не создаёт.

Pointer имеет вид `{"state":"present","sha256":"..."}` либо `{"state":"absent"}`.
Current требует retained owner receipt, confirmed — Verified owner receipt;
bytes/project/policy проверяет прежний owner reader. Оба absent допустимы только
для enrolled target без operations и с пустыми manifest/object inventories.
Один absent или потеря pointers при наличии history не означают empty target.
Это CAS observation, не live health/acceptance assertion.

## Граница Fleet consumer

Fleet связывает ответ с настоящей retained accepted Developer Complete chain и
raw Tracker receipt hash. Caller `accepted:true`, PR link, source SHA или успешный
CI не заменяют эту цепочку. R не утверждает Tracker acceptance.

После join Fleet может сохранить closed `forgeTaskDelivery` JSON и canonical
outer variables hash до delegation. Fresh Auth/Tracker/current child, task lease,
artifact/policy/CAS и publication проверки остаются обязательными перед effects.
R observation не исполняет pipeline/deploy/rollback, не обновляет lease, не mint-ит
child и не разрешает production. Новых endpoint кроме этого GET, scopes, issuer,
queue, таблиц, migrations, controllers или Hermes changes нет.

## Qualification и публикация

Historical source `26b73e0` имеет 15/15 focused Linux cases, fmt, typed compile,
strict Clippy и actual Rust OpenAPI export; PG-selected0. Это не положительная
SQL-интеграция и не qualification новых selection/name changes.
[Неизменённый historical receipt](evidence/NAMESPACE_DEVOPS_OWNER_20261019.md).

Merged main `ab2fc7c` содержит последующую квалификацию baseline: 372 unit,
два actual PostgreSQL/HTTP cases и generated API/hosted gates проходят на
head `0c8572a`. [Протокол merge-кандидата](evidence/NAMESPACE_DEVOPS_OWNER_20261019.md#последующая-квалификация-merge-кандидата)
подтверждает baseline R SQL/auth/custody, но не новый owner selector.
В follow-up сохранены DB-valid ciphertext, JWKS и preparation/submission claim
под живым embedded lease до terminal ACK; positive R fixture дополнен selection.

Текущий owner-selection/artifact-name follow-up требует нового 13-case gate:
**pending, PASS не заявлен**. Предыдущий PG run завершился ошибкой; его failed
evidence не переписывается и не считается PASS после source fixture fix.
Projection tests вызывают настоящий selector/observer SQL в private TEMP tables
с rollback, но не заменяют actual R HTTP/PG auth/custody case.

Rust router и новые schemas зарегистрированы в source. Committed OpenAPI пока
содержит прежний target без selection; штатная Rust generation/client refresh
для нового поля и OwnerDeliverySelection schemas ещё pending. Отсутствовавший
`artifact_names` re-export в `backend/src/domain.rs` добавлен при интеграции;
его Rust compile qualification также pending.
До отдельного результата нельзя заявлять current compile/API parity.
Production image, установка, live Auth/R/OCI и Fleet publication — отдельная приёмка.
