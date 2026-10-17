# Task Workspace И Candidate Git: Локальные Проверки

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
применённая repository-sync0093 в другой линии требует отдельного опубликованного
foundation и combined-history upgrade proof; ignore-missing запрещён.

Base introspection expiry metadata и Tracker source binding являются явными
зависимостями. Child renewal, legitimate server readers, окончательные delivery
receipts и live rollout остаются отдельными gates. UI, принятые образы, чужие
данные, production и default branch приложения не менялись. PR остаётся Draft.
