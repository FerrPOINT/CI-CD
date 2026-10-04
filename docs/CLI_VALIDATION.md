# Проверка CLI CI/CD

## Принятая поставка sdlc1 — 2026-10-03

Backend принят: `sha256:ce90e423cb2fb1cdb1e7d19c3f2eb2a0d2354c0f4ce86ee8d7e232b7c23927db`. Проверенный code source
`32e33aaf268df14111ef9f1df1b0c4846aa7c6b2`, опубликованный Base pin
`9408802dfa978cba2f67162a49adca6f65851b01`, Rust 1.88.0, locked dependencies.
Изменения main после этого source, включённые в документационный PR, не меняют
backend/CLI tree. SHA документационного merge проверяется отдельно от build source.
Предыдущий baseline image: `sha256:173e2ec9c9df40b5a72a020508fff8423a284bc79cf38fb48cc8aa9c5eaac8a2`.

repository/Git/PR, linked/unlinked merge, protected branch и Git conflict, variables, manual start/play, cancel/retry, attempts/log pages/artifacts, secret stdin, environment/deployment/approval/rejection/rollback. Pipeline wait проверен для обоих порядков HTTP/wait timeout, задержек headers/body, success/failed/canceled, без cancel и автоматического transport retry.

Установка и checksums: [CLI_INSTALL.md](CLI_INSTALL.md).
Все более ранние dated sections ниже — исторические проверки и blockers,
снятые этой поставкой, если явно не указано сохранённое ограничение.

### Каталог миграций и опубликованные проверки

Каталог 1–38 совпал с ledger fresh copy, включая точные исторические 36/37 и
CRLF bytes migration 38 (SHA-384
`875f127c9f0c51477f61cee89816edea0168d5cf842f3bdbfce5d683df033af224f5619035439f84359f671aee2981ce`).
[PR #83](https://github.com/FerrPOINT/CI-CD/pull/83), merge
`32e33aaf268df14111ef9f1df1b0c4846aa7c6b2`;
[точный post-merge CI](https://github.com/FerrPOINT/CI-CD/actions/runs/37126708145)
— docs/backend/frontend/minimum-rust 4/4 PASS. Отказ VersionMissing(38)
воспроизведён до исправления. Fresh/upgrade/replay/invalid checksum и чтение
historical deployment/plan проверены без изменения ledger/data. Только точная
SQL compatibility migration и тесты опубликованы; Messaging SDK/полный Pulse
deployment код не переносились. Local: focused 5, workspace 164, server
PostgreSQL integration 63, target policy 4, CLI real API 2 в отдельной
forge_test_cli, integration Clippy, release/fmt/MSRV/OpenAPI, frontend 196,
contracts/compat/lint/build/packed Base/themes, docs 6 и secret scan — PASS.

### Сохранность, откат и границы

По отдельному решению пользователя принят новый чистый sdlc1, подготовленный
другой задачей после потери прежнего Docker/data. Утраченные старые данные и
images не восстановлены этой поставкой. Baseline отката — точные images нового
чистого стенда, сохранённые до обновления; восстановление утраченного старого
стенда не заявляется.

Свежий согласованный backup трёх БД и пяти фактических файловых volumes:
`20261003T142547-890aba`, 11 payloads с проверенными checksums. Перед ним
проверены нулевые active jobs/queue/leases; остановлены только три писателя.
Отдельная fresh-copy QA использовала backup `20261003T122738-8a92db`: restore
45/31/17 таблиц CI/CD/Task/Wiki, rows/sequences/schema/constraints/owners/grants
и hashes/ownership пяти volumes совпали. Эти volumes были пустыми на новом
baseline; сохранность файлов дополнительно подтверждена fixture upload/Git/
artifact/download сценариями, а не восстановлением потерянных старых файлов.

На копии запуск кандидатов, затем точных прежних images не изменил исходные
данные и схему. Старые images проходят 114 проверок и сохраняют четыре известные
ограничения: Task read/restore по ключу, Wiki legacy multipart replay и старое
CI/CD ограничение generic deployment только для Pulse. Поэтому откат возвращает
прежнее поведение, а не гарантирует исправленные CLI-сценарии. Неожиданных
отказов финальной репетиции нет. Первый rollout откатился из-за преждевременной
проверки Docker health `starting`; после исправления локального readiness wait
второй rollout принят. Рабочие данные не восстанавливались и ledger не правился.

Под workspace lock атомарно заменены только три image pins, затем последовательно
Task → Wiki → CI/CD через Compose `up --no-deps --no-build --pull never`.
Readiness: HTTP и Docker healthy, deadline 180 секунд на сервис. Откат: под тем же
lock вернуть три сохранённых pins и пересоздать эти сервисы в том же порядке;
не выполнять автоматический restore рабочих данных. Защищённые backups,
runtime-before и rollout manifest сохранены локально; секреты не публикуются.

Наблюдение: 900 секунд, 31 sample каждые 30 секунд, readiness 200/healthy,
0 restarts, 0 новых ERROR в обоих потоках логов. Остальные контейнеры/mounts
не менялись нашим rollout. Во время наблюдения отдельная задача обновила
admin-api/admin-web/ai-runtime sdlc2; их images сверены с её build/apply receipt,
mounts сохранены. Это отражено отдельно, глобальная неизменность sdlc2 за весь
интервал не заявляется. Наши keys/runtime fingerprints остались неизменными.

QA-кандидаты: 123 основных + 10 дополнительных проверок — PASS. Live: 123
успешные проверки и 6 focused checks — PASS. Отдельная ранняя попытка template
`type=page` получила корректный validation 400: ошибка fixture, исправлена на
поддерживаемый `release_note`; успешный повтор записан отдельно, исходный отказ
не скрыт. JSON/204, confirmations, stdin, access/validation/conflict, redaction,
transport timeout, no-clobber и cleanup проверены регрессиями и CLI-приёмкой.
Execution states задавались собственными fixtures/API; production runner и
внешний deploy не сертифицируются. Pulse — обычный тестовый repository/PR/pipeline
в CI/CD, отдельного постоянного стенда нет. Собственные PAT отозваны, QA Compose
ресурсы и временные keys удалены; принятые и прежние backend images сохранены.

## Исторические проверки

## Каталог исторических миграций — 2026-10-03

Baseline опубликованного main: `6fc5c6677ceeca86b7a7a35138844c3423fa18eb`;
чистый Base pin `9408802dfa978cba2f67162a49adca6f65851b01`, Rust 1.88.0.
PostgreSQL 17.6 прочитал защищённый исторический backup от 2026-10-01:
все 37 checksums совпали с исходниками с учётом неизменных `.gitattributes`
(часть ранее применённых SQL-файлов сохраняет CRLF). Migration 36/37 взяты
байт-в-байт из существующего коммита, без переноса Pulse deployment/runner API.
Подтверждены request_key UUID, unique index и historical config_source.
Найдены 12 deployment plans; активных Pulse deployments, jobs, queue entries
или leases в этой копии нет. Это не инвентаризация текущего sdlc1.

Четыре новые PostgreSQL regression tests сначала упали на каталоге 1–35,
включая `VersionMissing(36)`; после добавления 36/37 — 4/4 PASS. Проверены
fresh schema, upgrade с 35, повтор без изменения history, неверный checksum,
сохранность request keys и hashes и чтение исторических plan/deployment через
существующие API. Тесты входят в обычный `integration_db` workflow gate.

Повторены fmt, minimum Rust, workspace/all-target Clippy, integration Clippy
для server/CLI и locked release workspace build — PASS. Workspace 164,
PostgreSQL integration_db 62, target policy 4, CLI real API 2 — PASS.
OpenAPI dump совпал с опубликованным контрактом. Frontend tree не менялся:
Node 22.20.0 / pnpm 10.28.1, frozen install, contracts/compat, 196 tests,
lint/build, generated-file check, packed Base consumer и themes — PASS.
Docs regression tests 6, docs validator и secret scan — PASS.

На отдельной восстановленной БД native API-only startup с каталогом 1–35
воспроизводит `VersionMissing(36)`, с 1–37 — readiness 200. После старта
и остановки все 44 таблицы, sequences, schema, owners/grants неизменны.
Runner, SMTP и внешние deploy targets отключены. Это не проверка прежних
runtime image IDs и не рабочий rollout.

CLI/API/roles и generic deployment сценарии сохранены. Pulse остаётся
тестовым репозиторием/pipeline внутри CI/CD; отдельный постоянный Pulse-стенд
и специальный deploy runner не создавались. Rollout по-прежнему требует
восстановленного sdlc1, свежего backup и проверенного previous-image rollback.

## Исходная CLI-приёмка — 2026-10-02

Проверено 2026-10-02 в изолированном task checkout `feat/cli-workflows`.

## Среда и обязательные gates

Ubuntu WSL, rustc 1.88.0 (6b00bc388 2025-06-23), Node 22.20.0 / pnpm 10.28.1, Python 3.12.3. Проверено после объединения с актуальным продуктовым `main`, с чистым опубликованным Services Base `c083783a37791e277db796361203884b87828a7d`, закреплённым в `.base-revision`. Принятая в `main` политика pinned Base, Cargo `--locked` и pnpm `--frozen-lockfile` сохранена. Base и исходные dirty checkout этой задачей не изменены.

Backend: fmt, workspace/all-target Clippy с `-D warnings`, workspace tests, отдельный MSRV check (`cargo check --locked --workspace --all-targets`), OpenAPI drift и release workspace — успешно. Workspace: **164 passed, 0 ignored**, 0 failed. Дополнительно проверен штатный параллельный workspace invocation CI/CD.

```bash
cd backend
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace -- --test-threads=1
cargo build --locked --release --workspace
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cicd cargo test --locked -p cicd-server --features integration --test integration_db -- --test-threads=1
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cli cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cicd cargo test --locked -p cicd-server --features integration --test integration_target_policy -- --test-threads=1
```

Docs validators и существующие CI-contract tests проходят. Frontend: install с `--frozen-lockfile`, OpenAPI check/compat с `origin/main`, tests, lint и build — успешно. Дополнительно проходят неизменность generated contracts/lockfile, packed Base consumer и effective themes (dark/gray/light) на built preview. Task Tracker дополнительно typecheck; Task Tracker/Wiki — предусмотренный format check. Frontend tests этого продукта: 196.

## Регрессии и проверенные сценарии

- `download_transport`: обрыв Content-Length, timeout после headers, JSON/text errors, exit 3/TRANSPORT_ERROR/status null, redaction/request ID, сохранность destination с overwrite, отсутствие частичного файла, success и no-clobber. До исправления regression возвращал exit 2.
- `wait_deadline`: оба порядка HTTP/wait timeout и равные ограничения, задержка headers и тела, flag/env с приоритетом flag; exit 3 и правильный TRANSPORT_ERROR/WAIT_TIMEOUT, запросов retry/cancel нет. До исправления более короткий HTTP timeout игнорировался.
- Existing workflows дополнительно проверяют queued → running → success, failed/canceled с последним JSON, sleep в пределах deadline, variables, manual jobs, attempt logs, secrets stdin и repository/PR.
- PostgreSQL server integration suite (58 tests), policy suite (4 tests) и CLI real API (2 tests) выполнены раздельно: `forge_test_cicd` и `forge_test_cli`, PostgreSQL 17.6. Real CLI использует production HTTP handlers, временные Git/artifact directories; fixture задаёт execution states. Production runner/deploy не запускаются.

Существующие проверки file/stdin, pages, JSON/204, access/validation/conflict, transport timeout, credential redaction и download no-clobber сохраняются и проходят. Новые regressions воспроизвели замечания на исходной ветке, затем прошли после исправлений.

## Границы подтверждения

Проверки используют только fixture данные и собственные временные ресурсы. Постоянные Compose-группы, runtime images, volumes и production deployment не менялись. Windows native linking недоступен (`link.exe`); Rust gates выполнены в WSL. Новых endpoint, миграций или изменений Services Base нет. Эти проверки были выполнены до merge; статус main и живая приёмка приведены ниже. Deploy не выполнялся.

Описание команд, configuration, input/output/errors и ограничения: [CLI.md](CLI.md).

## После merge и приёмка sdlc1 — 2026-10-02

Merge commit: `bd728e9e7dd57c14331d1341bb9a74614ac5bfc4`. [Post-merge CI](https://github.com/FerrPOINT/CI-CD/actions/runs/37000164147): docs/backend/frontend/minimum-rust — 4/4 success на этом точном SHA; содержимое merge tree совпало с reviewed head. CLI release binary собран отдельно с закреплённым Base; checksum и установка: [CLI_INSTALL.md](CLI_INSTALL.md).

На собственном private repository выполнены Git push двух веток/tag, refs/tree/blob/tags/commits/compare, PR create со stdin/get/list/close/reopen. Merge PR на работающем backend возвращает HTTP 500 internal_error; его image revision — 6e5c557fa479f622a1f8543a80182f8a3282e516, отличная от проверенного main. Выполнены project/pipeline create, variables и replay, manual play, прежний start, fixture status/log mutations, success/failed/canceled wait, attempts/logs-page выбранного attempt, artifact upload/download/no-clobber, cancel/job retry/pipeline retry, secret stdin/delete, environment/deployment metadata, approval/rejection и rollback. Wait timeout 1 s завершился WAIT_TIMEOUT/exit 3 в пределах 5 s и сохранил активный pipeline. Terminal failed/canceled возвращают 3 и JSON snapshot в stdout, без stderr. Read-only PAT получил 403. Execution states задавались на собственных fixture jobs; production runner/deploy приёмка не заявляется. Активные собственные pipelines отменены, project удалён через публичный API (в CLI команды удаления проекта нет), repository удалён CLI.

Проверка выполнялась с временными Central Auth PAT, ограниченными тремя продуктами; read/write и read-only tokens отозваны после прогона. Значения tokens/credentials не сохранялись в логах или артефактах. Исходные dirty checkout, постоянные Compose-группы, runtime images/pins и volumes не изменялись. Fixtures использовали реальные API и PostgreSQL работающего sdlc1, но только собственные project/repository/space и файлы. Ошибки исправленного smoke (имя флага search, when: manual, начальный deployment status и вывод terminal wait) отделены от воспроизведённых отказов runtime.

**Статус:** CLI main/CI проверен; полная совместимость с текущим sdlc1 не принята. Требуется отдельная сверка/обновление backend до согласованных main-кандидатов и повтор блокирующих операций. Публичный release не объявляется готовым.

## Подготовка backend-кандидатов — 2026-10-02

Task checkout обновлён merge актуального опубликованного main без переписывания истории. Текущий продуктовый pin Base: `9408802dfa978cba2f67162a49adca6f65851b01`; он отличается от Base предыдущей CLI-поставки. Чистый Base checkout проверен через verify_base_revision. Предыдущие результаты не подменяют новую проверку этого pin.

Повторно выполнены 6 documentation regression tests и штатный documentation validator — PASS; git diff --check — PASS. Новая проверка backend/frontend, PostgreSQL fixtures, сборка образов, restore rehearsal и live acceptance не завершены: C: заполнен, Docker containers API возвращает HTTP 500, затем WSL стал возвращать E_UNEXPECTED. Попытка локального PostgreSQL старта завершилась с exit 1 без подтверждённого запуска и без выполненных cargo gates.

Runtime pins не записывались, образы не заменялись, миграции и restore не запускались, постоянные сервисы не перезапускались. Новых PAT и API fixtures не создавалось. Исправность текущего runtime и очистку временного WSL build root нужно подтвердить после восстановления окружения. Эта попытка не устранила ранее описанные live-блокеры и не подтверждает готовность новой поставки.

## Возобновлённая проверка кандидатов — 2026-10-02

Проверены опубликованный main `ae0fcf45c173c982a017a67e8a408a3b0702abfc` и замороженный task source `ae43dcc134209530f8d9604fd24c14d987b76ec4`; backend tree task source совпадает с main. Все продукты используют чистый опубликованный Base `9408802dfa978cba2f67162a49adca6f65851b01`. Операционные инструменты backup/restore сохранены отдельным snapshot локального Base с hashes/status: они не представлены как чистый опубликованный Base.

Rust 1.88.0: fmt, minimum Rust (locked workspace/all-targets), Clippy с `-D warnings`, workspace tests, OpenAPI drift и locked release workspace build — PASS. 164 workspace tests; PostgreSQL integration_db 58/58, integration_target_policy 4/4, CLI real API 2/2; Clippy CLI с feature integration — PASS. Node 22.20.0 / pnpm 10.28.1: frozen install, OpenAPI check/compat, frontend tests (196), lint/build, generated-contract/lockfile check, packed Base consumer и effective themes — PASS. Task Tracker typecheck и предусмотренный format check Task Tracker/Wiki — PASS. Первоначальный Wiki format failure локализован в CRLF export; canonical LF export проходит без изменения исходников.

Собран продуктовый Dockerfile с Rust 1.88.0 и locked dependencies из canonical LF source; image ID `sha256:38efbc70e620fd9b2e9d86d8a172bb5793c8f253ebac41178e0f716c7e09d2a8`. Branding builder не применялся. Штатный пользователь, бинарник, runtime libraries, migrations (где поставляются файлами) и доступность собственного uploads/storage каталога проверены. Runtime pins не изменялись.

Merge собственного PR в репозитории без связанного проекта проходит. Repository/Git/PR, pipeline variables/replay, manual play и start, wait/attempts/log pages, artifact download/no-clobber, cancel/retry, secret stdin, deployment approval/rejection/rollback проходят на отдельной пустой PostgreSQL fixture. Execution states задавались через fixture control API; runner и внешние deploy targets отключены.

Общий изолированный CLI-прогон: 123/123 PASS; дополнительный прогон custom-field values и Wiki partial recovery: 10/10 PASS, включая повторные auth checks. Использованы временные QA identity/PAT и отдельный native Auth fixture из чистого pin Base; PAT отозваны. Backends работали в internal Docker network, без Docker socket, runner и внешних deploy targets. Human/JSON confirmations, пустые ответы, stdin, доступ/валидация/conflict, downloads/no-clobber проверены в QA и регрессиях. Оба порядка HTTP/wait timeout, задержки headers/body, redaction и cleanup download temp files проходят subprocess/HTTP tests.

Scoped restore исторического backup от 2026-10-01: три БД (Task Tracker 31, CI/CD 44, Wiki 17 таблиц), rows/sequences/constraints/owners/grants и четыре файловых volumes совпали с evidence; hashes и права 173 файлов совпали. После старта Task Tracker/Wiki данные и схема не изменились; все 8 SQLx checksums Wiki совпали с canonical source. Это не свежая согласованная копия текущего sdlc1.

**Блокеры rollout:** прежние постоянные runtime/images/volumes отсутствовали в Docker после восстановления окружения; их восстановление не входит в этот CLI rollout. Историческая CI/CD БД содержит применённые миграции 36/37, которых нет в опубликованном main (main содержит 1–35). Кандидат отказывается запускаться с VersionMissing(36). Существующий локальный commit `3aa12a4cdf077db720ef1e2e4c715db7fa443620` содержит эти файлы, но не включён в кандидат. Ledger миграций и данные не удалялись и не переписывались. Требуется отдельный план согласования опубликованного кода со схемой, свежий backup и проверенный rollback на прежние image IDs.

Исторические Wiki idempotency records не переписывались. Новый multipart hash подтверждён для записей после обновления; replay старых records с прежним raw multipart hash не сертифицирован, автоматической замены ключа нет. Wiki QA документы/space архивированы через API; до удаления временной QA-копии attachments/evidence/audit/replay retention rows оставались в ней. После проверки удалены только собственные QA ресурсы: 8 контейнеров, 7 volumes и 2 сети. Исходный protected backup и candidate images сохранены; исходные retention records не переписывались. Production pins, signing keys и volumes не изменялись.

**Статус:** исходники и три CLI-кандидата проверены; обновление sdlc1, прежние image rollback, живая приёмка обновлённого стенда и 15-минутное наблюдение не выполнены. QA CI/CD на пустой БД не заменяет приёмку сохранённых данных. Native Windows/macOS/musl, version bumps, tags и публичный release не входят в поставку.

## Совместимость с каталогом 38 — 2026-10-03

Отдельная задача восстановления предоставила чистый sdlc1 вместо удалённого
старого Docker. Его CI/CD image
`sha256:173e2ec9c9df40b5a72a020508fff8423a284bc79cf38fb48cc8aa9c5eaac8a2`
содержит применённые migrations 1–38. Защищённая свежая согласованная копия
трёх БД и пяти файловых volumes проверена restore в собственные пустые QA
volumes: rows, sequences, columns, indexes, constraints, owners/grants и
файловые hashes/права совпадают. Незначительная разница скобок в одном CHECK
после pg_dump/restore отдельно проверена на эквивалентность выражений.

Кандидат из опубликованного main `a3c54fd36cf4f253fe51934ecf65b236a6d49cd6`
воспроизводит VersionMissing(38) на этой копии без изменения данных. Добавлена
точная существующая migration 38 из baseline image: 1057 bytes, CRLF, SHA-384
`875f127c9f0c51477f61cee89816edea0168d5cf842f3bdbfce5d683df033af224f5619035439f84359f671aee2981ce`.
Ledger и исторические данные не переписываются. Публикуется только каталог
схемы; messaging SDK, publisher/subscriber, новые API и изменения Base не входят.

Регрессия сначала воспроизвела VersionMissing(38) на каталоге 1–37. На
PostgreSQL 17.6-alpine новый каталог проверен для свежей БД, upgrade с 35 и 37,
replay ранее применённых 36/37/38, отказа при неверном checksum и сохранности
исторических deployment plans и outbox/pipeline records. Pipeline читается
через прежний API после replay. Тестовые БД forge_test_cicd и forge_test_cli
изолированы; временный Compose-проект не подключён к рабочим данным.

Rust 1.88.0, locked dependencies, чистый опубликованный Base
`9408802dfa978cba2f67162a49adca6f65851b01`: fmt, minimum Rust, workspace Clippy,
workspace tests, PostgreSQL integration_db (63), integration_target_policy (4),
CLI real API (2), server/CLI Clippy с feature integration, release workspace
build и OpenAPI drift — PASS. Эти результаты относятся к каталоговой поправке,
а не к полной приёмке нового runtime.

Node 22.20.0 / pnpm 10.28.1 на неизменённом frontend tree: frozen install,
OpenAPI check/compat, 196 tests, lint/build, generated contract/lockfile check,
packed Base consumer и effective themes — PASS. Documentation regression tests
(6), штатный documentation validator, secret scan и git diff --check — PASS.

Baseline pins пока не изменены. Полная QA новых образов, репетиция отката на
точные images нового baseline, live acceptance и 15-минутное наблюдение остаются
обязательными этапами. Исторические доказательства предыдущих прогонов выше
сохраняются отдельно и не подменяют эту проверку.
