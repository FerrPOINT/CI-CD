# Task Workspace И Candidate Git: Локальные Проверки

## Frontend f259/UIe075: 18 октября 2026

Exact source `f2597dcdb640e62ecdb93c7c2a68b5d11cb31783` включает upstream124;
backend SDK2ad сохранён отдельно от UI pin
`e0759e070326c905f365182d0107f8c87a2e67f6`. Node22.20.0/pnpm10.28.1 source-only
проверка подтвердила frozen/offline dependency graph, exact412 файлов установленного
UI package, generated-client parity и неизменённый Base compatibility checker
против main `00ba828f5f18f9d8d8f249f18e01ff025a5c6a93`.
Все8 целевых Vitest случаев из users, projects и app-shell catalog isolation PASS,
без failed/skipped. Inputs неизменны, cleanup подтверждён; browser/build/live не
выполнялись. Receipt SHA256:
`1285c0e42b7cf0c1bef5e7e46e172b99159b4e0234850082084405a0c1df11ca`.

Отдельная TypeScript-only проверка того же f259 использовала существующие
dependencies immutable builder, readonly Git-exported source и `tsc --noEmit`.
Checksum checks до и после связали mounted declarations/OpenAPI и cached configs/
lock с Git bytes. Result: status=passed, exit_code=0, cleanup_verified=true,
inputs_cleaned=true; build=false, install=false, live=false. Receipt SHA256:
`cdeb06d1ce432f24bb33629cde44ad55eda929830796480c79f18561d753cd79`.

Это точное reuse старого Vite production build, не сборка нового HEAD. Все67
production `frontend/src` blobs и прочие frontend runtime/config/package/lock inputs
совпадают с исходным build tree `65b70989c47db007862aa168e622c8fb11a55dde`.
Отличаются только `projects.test.tsx`, generated `schema.d.ts` и OpenAPI YAML;
новые declarations проверены указанным TSC, а runtime JavaScript не менялся.
Сохранены первоначальные identities и build args:

- Builder `sha256:b6ea2ec9e257675aeb086e8cc724813c143a1cd72e34f044ec2164dd5b6da8d4`.
- Web `sha256:ace1bac4f3f2f0f45f83b2dd8947e73ab5d150d5c61d8a0f8c5ac956a9378e49`.

Original build receipt не переписан; его SHA256:
`563f0eb9708ed12aedd9e81656eff08ecd233c59ed9aabfdf0e972c5f9d69c2a`.
Packed Base consumer/build/themes также подтверждены отдельным
[hosted PR124 run на e026ac2](https://github.com/FerrPOINT/CI-CD/actions/runs/39068410430/job/117500760204),
а не объявлены заново выполненными на f259. Это ограниченная frontend closure
qualification, не полный repository gate или production-ready source.

Две исправленные catalog expectations до0094 ещё требуют своих exact PG результатов:
`migration_catalog_contains_exact_historical_36_and_37` и
`migration_catalog_fresh_and_prior_35_or_37_upgrade`. QA42 на source828cf ниже
остаётся самостоятельным Rust evidence и не подменяет эти два случая. Новый
backend image/Compose, полный release gate и live producer/deployment acceptance
по-прежнему не закрыты.

## Explicit Routes: Проверенный Source 828cf

`828cf811a186109e4d5a4e2d64b7fa26538e4c3a`, SDK2ad, прошёл один новый целевой
локальный QA: fmt, locked/offline metadata, typed lib/integration compilation,
37 lib cases и5 PostgreSQL cases, strict affected Clippy и Rust OpenAPI export.
Итого42/42 PASS без ignored/skipped. Это не полный workspace/release/native gate.

Включены точные исправленные populated-history и Task Git/RepoSync сценарии,
обе native hook проверки через новый callback, defensive legacy payload rejection,
registered-reader права, source query/wire и namespace/cancellation interleavings.
Новое чтение source находится на `GET /namespace-repositories/{id}/source`, а task
callback на `POST /internal/task-git-check`; legacy object contracts сохранены.
Generated-client parity, TypeScript и неизменённый Base compatibility checker
проходят после фактического Rust export. Схема не редактировалась вручную.

Новый receipt: runner_exit0, cleanup_verified=true, journal_phase=cleaned;
source inputs также удалены. SHA256:
`5a8b77cbe46eb351a46ad10cb2a46d1cc792bb044e8912819181b933de73b0bc`.
Исходный failed e564 receipt ниже остаётся неизменным. Новый результат не
переписывает его и не доказывает установленный Auth/Forge или выдачу credentials.

## Исторический Combined Main: Тринадцать Случаев

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
Этот старый прогон не квалифицирует последующие исправления; их новый результат
приведён выше.

Исходный result остаётся runner_exit1, cleanup_verified=true,
journal_phase=cleaned. SHA256:
`117f53d08090f782a6709927e28ae9327cdb357438144cbe70957a5dfa659896`.
Его логи, receipts и исходные отказы не переписаны.

На том checkpoint generated-client parity и TypeScript проходили, а Base API
compatibility отклонял object-to-union изменения request `/internal/git-push` и
response `/namespace-repositories/{id}`. Новый проверенный source выше закрывает
этот блокер явными маршрутами.
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

Остаются полный repository release gate, image/Compose и настоящий scoped
producer/deployment сценарий. Целевые Clippy/codegen/compatibility и populated
history закрыты новым828cf QA, не старым checkpoint9b.
Единственная новая миграция0094 не меняет свой SQL при перенумерации. Уже
применённая repository-sync0093 теперь опубликована в main и совпадает с прежним
foundation по точному Git blob/checksum. Целевой populated-history upgrade до0094
подтверждён QA828cf. Полная repository qualification и отдельные fresh/prior-schema
catalog проверки этим не заменяются; две исправленные catalog expectations ещё
ожидают целевого прогона. Ignore-missing запрещён.

Base introspection expiry metadata и Tracker source binding являются явными
зависимостями. Child renewal, legitimate server readers, окончательные delivery
receipts и live rollout остаются отдельными gates. Проверки не меняли принятые
образы, чужие данные, production или default branch приложения. PR остаётся Draft.
