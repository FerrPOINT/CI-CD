# Точный owner selector для первой публикации DevOps

Source-only companion к зарегистрированному R-readback
`GET /api/v1/namespace-repositories/{id}/devops-evidence`.
Новых endpoint, scope, issuer, queue, миграций или SAT fallback нет.

В существующий приватный `DeliveryPolicy` добавлено optional поле:

```json
"deliverySelection": {
  "version": 1,
  "action": "deploy",
  "stageName": "build",
  "jobName": "package",
  "artifactName": "dist__bundle.tar"
}
```

Для OCI это поле находится в существующем `checks`. Поле входит в полный
типизированный policy hash. Без него прежняя сериализация и hash остаются
прежними, R не возвращает selection, Fleet не получает first-deploy action.
Смена/enrollment policy остаётся явной защищённой owner-процедурой; данный
пакет не изменяет установленные policy, owner metadata или manifests.

Имена сравниваются буквально с сохранёнными stage/job/artifact names исходного
Developer pipeline. Resolver проверяет latest attempt и retained artifact,
ровно один declared path с тем же кодированием имени, затем точное совпадение
с уже проверенным candidate inventory. Ноль/два результата, stale attempt,
дубликаты и неоднозначное кодирование path запрещены, первого элемента нет.
Фильтры параметризованы; SQL только SELECT в существующей R-транзакции.

R возвращает optional `target.selection`: schema
`forge/owner-delivery-selection/v1`, action `deploy`, sourcePipelineId,
configSha256, planSha256 и полный CandidateArtifactEvidence. Это observation
policy selection, а не writer permit. Registry/namespace authorization,
catalog fence, фактический retained original receipt и target ownership
сохраняются. Child issuance/renewal и execution consumer по-прежнему требуют
актуальные Auth/Tracker/source/artifact/policy/CAS проверки.

Rollback не выбирается автоматически по ошибке или наличию confirmed pointer.
Новый selector содержит только deploy. Fleet может replay уже сохранённой
явной rollback-команды, но данный пакет не создаёт first-rollback authority.

Source-only проверки: четыре Rust unit selectors и один ignored PostgreSQL
projection selector в owner_selection_tests.rs написаны, но Cargo/PG не
выполнялись. PG case проверяет exact/latest-attempt/duplicate/path-alias SQL
в connection-private TEMP tables с rollback, не заменяет настоящую R HTTP/PG
проверку auth/custody из существующего fixture. До qualification нужны оба scope.
Main регистрирует OwnerDeliverySelection и OwnerDeliverySelectionAction при
штатной Rust OpenAPI/client generation; generated schemas вручную не правятся.

## Canonical artifact names

`domain::artifact_names::artifact_name_from_declared_path` сохраняет прежнее
runner-кодирование: `/` и `\` превращаются в `__`, без другой нормализации.
Embedded и external runner используют общий `declared_artifact_names` до
чтения/upload: duplicate paths и разные paths с одним encoded name запрещены.
Проверки filesystem paths остаются прежними; HTTP protocol/headers не меняются.

Общий hook заменяет literal `ar.name=declared.name` в `candidate_jobs`:
SQL получает все имена именно latest attempt без DISTINCT и проверяет retention,
expiry, sha presence, positive size; Rust сопоставляет через общий mapping.
Дубликаты observed names, отсутствующий output и declared aliases запрещены.
Исторические attempts не используются вместо current artifact; строки не правятся.
Owner selector использует тот же helper вместо второго SQL replace expression.

Source-only inventory дополнен пятью pure domain cases и ignored PG case
`candidate_jobs_nested_names_stale_rows_duplicates_and_aliases_fail_closed`.
Последний вызывает настоящий observer SQL на transaction-local projection tables
с rollback, проверяя nested positive, literal/basename mismatch, duplicates,
aliases, expired/purged/null-sha/empty artifacts и смену latest attempt.
Существующий selector PG case также проверяет slash/backslash/repeated aliases.
Это не результаты запуска и не замена фактической R HTTP/PG qualification.
