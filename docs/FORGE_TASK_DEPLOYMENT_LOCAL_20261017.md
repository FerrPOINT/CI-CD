# Task Workspace И Candidate Git: Локальные Проверки

## Combined Main: Тринадцать Целевых Случаев

Exact `e5641b40f580700a149b7909fa7fafb80ba0a88f` включает опубликованный
RepoSync main с неизменной исторической0093 и SDK2ad. В одном локальном QA
проходят fmt, locked/offline metadata, typed lib/integration compilation,
восемь auth/readback units и три native Git namespace/cancellation PG cases.
Rust OpenAPI export также получен. Всего11/13 тестов PASS; это не aggregate PASS.

Два PG-теста отказали. Исторический0091 seed ошибочно использовал текущий0094
writer; новый Task Git fixture не публиковал canonical group/slug alias.
Последний отказ не доказывает обход MutationGuard. Опубликованный test-only fix
сохраняет проверки истории, refs, locks и timeouts, добавляя точный historical
INSERT, discovery404/200 assertions и безопасную диагностику раннего Git exit.
Отдельно исправлены семь фактических strict-Clippy diagnostics без allow-атрибутов.
Исправленный исходник ещё не имеет нового Rust/PG/Clippy PASS.

Исходный result остаётся runner_exit1, cleanup_verified=true,
journal_phase=cleaned. SHA256:
`117f53d08090f782a6709927e28ae9327cdb357438144cbe70957a5dfa659896`.
Его логи, receipts и исходные отказы не переписаны.

Generated OpenAPI/client parity и TypeScript проходят. Base API compatibility
отклоняет object-to-union изменения request `/internal/git-push` и response
`/namespace-repositories/{id}`; исправление legacy-контрактов остаётся gate.
Ни compatibility waiver, ни фиктивная схема вместо настоящего Rust export не
используются. Последующие API-правки требуют собственного актуального codegen.

## Точный Source

Кандидат `9b5d4c633195bc5020d58504ee1bbec5e8ad2a87`, SDK
`2ad0834d06d3aa703fa1791a932bfc76650c8e42`, PR122. Проверки локальные,
временные ComposeHelper v2 проекты, isolated PostgreSQL без host-порта,
закреплённый Rust1.88/J1/offline и неизменные lock-файлы. Runtime стенда не менялся.

## Выполнено

Типизированная компиляция server lib/integration и CLI test targets, fmt и metadata
проходят. Основной прогон выполнил64 теста без отказов:

- Deployment parser7, owner13, remote custody23.
- Candidate Git server15 и CLI3.
- Workspace deployment PostgreSQL admission3.

Оставшийся ordinary task PostgreSQL/embedded-shell сценарий был найден точно,
но проверочный скрипт ошибочно сравнил `1 test` с `1 tests`. Поэтому исходный
aggregate receipt остаётся runner_exit1, а этот случай в нём не выполнен.
SHA256 исходного receipt:
`73c06a1b04c1361781a9501058df9af261325c8d445fdbac7e614256c61fd38f`.

После исправления только grammar assertion отдельный existing-runner follow-up
на том же source выполнил ровно этот один тест:1 passed/0 failed/0 ignored.
Его runner_exit0, SHA256 receipt:
`08df57b69ce6261bee18451738df0fbc45bc6480f0e36210208b9b7a7371aba9`.
Итого65 выбранных случаев подтверждены двумя честно разделёнными запусками,
а не переписанным общим PASS. Оба проекта очистили только собственные disposable
ресурсы; cleanup_verified=true и journal_phase=cleaned.

Ранее69 host-тестов подтверждают подготовку runner/transport, не реальные
Auth/Tracker права. Git hook fixtures используют настоящий Git и synthetic
HTTP verifier; PG admission использует настоящий isolated PostgreSQL и fake peers.
Это не native deployment или сквозная приёмка приложения.

## До Выпуска

Остаются affected Clippy, regenerated OpenAPI/client parity, populated migration
history case, image/Compose и настоящий scoped producer/deployment сценарий.
Единственная новая миграция0094 не меняет свой SQL при перенумерации. Уже
применённая repository-sync0093 теперь опубликована в main и совпадает с прежним
foundation по точному Git blob/checksum. Combined-history upgrade proof остаётся
отдельным gate; ignore-missing запрещён.

Base introspection expiry metadata и Tracker source binding являются явными
зависимостями. Child renewal, legitimate server readers, окончательные delivery
receipts и live rollout остаются отдельными gates. UI, принятые образы, чужие
данные, production и default branch приложения не менялись. PR остаётся Draft.
