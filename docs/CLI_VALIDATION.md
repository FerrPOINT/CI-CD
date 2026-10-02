# Проверка CLI CI/CD

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

Исторические Wiki idempotency records не переписывались. Новый multipart hash подтверждён для записей после обновления; replay старых records с прежним raw multipart hash не сертифицирован, автоматической замены ключа нет. Wiki QA документы/space архивированы через API; attachments/evidence/audit/replay retention rows остаются до штатного retention.

**Статус:** исходники и три CLI-кандидата проверены; обновление sdlc1, прежние image rollback, живая приёмка обновлённого стенда и 15-минутное наблюдение не выполнены. QA CI/CD на пустой БД не заменяет приёмку сохранённых данных. Native Windows/macOS/musl, version bumps, tags и публичный release не входят в поставку.
