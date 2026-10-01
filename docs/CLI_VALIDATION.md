# Проверка CLI CI/CD

Проверено 2026-10-01 в отдельном task checkout `feat/cli-workflows`.

## Пройденные проверки

Среда: Ubuntu WSL, Rust 1.88.0, Node 22.23.3, pnpm 10.28.1, Python 3.12.3. Чистый опубликованный Services Base `main`: `c008bec701086d4f9201180ea5451f64e88ab519`; политика зависимости от `main` сохранена. Backend source snapshot сверён с task checkout. Проверки PostgreSQL выполнены в изолированных БД, включая Docker `postgres:17-alpine` (PostgreSQL 17.11).

Из `backend`:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cli cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cicd cargo test --locked -p cicd-server --features integration --test integration_db -- --test-threads=1
cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings
cargo build --locked --release --workspace
```

- Workspace: 158 тестов, 0 failed. CLI: 3 unit, 10 contract, 9 subprocess/HTTP workflows.
- Real API/PostgreSQL: 2 теста, 0 failed; auth/RBAC/PAT/redaction и полный рабочий сценарий CLI.
- Фильтры, страницы, variables, PR description stdin, secret stdin, JSON stdout/empty success, структурированные stderr, parser credential redaction и download no-clobber проверены subprocess assertions.
- `pipeline wait`: polling queued→success, failed/canceled с ненулевым exit code, ограниченный deadline с медленным HTTP-ответом или чрезмерным poll interval, отсутствие cancel при timeout.
- Реальный API: repository create/refs/tree/blob/tags/commits/compare/delete, PR create/list/get/close/reopen/merge, pipeline variables/replay/cancel/retry/wait, manual job play и прежние start/fail, job retry/attempt logs, artifact download, protected environment и deployment approve/reject/rollback.
- Merge PR до создания связанного CI-проекта проверяет исправление NULL aggregate в protected branch lookup.

## Границы подтверждения

Real API запускает production handlers с настоящим PostgreSQL, отдельными repository/artifact directories и временными Git fixtures. Runner не запускается: manual flag и финальные execution/deployment statuses задаются тестовой fixture; лог/artifact создаются через HTTP. Это проверка управления и API-контрактов, а не исполнение production job или deploy. Отдельно выполнен полный существующий server PostgreSQL integration suite: 58 tests, 0 failed; он не входит в workspace count. Вызов явно выбирает пакет `cicd-server` и feature `integration`.

UI, Docker images и runtime окружения продуктов не изменялись. Windows native linking недоступен (`link.exe`), поэтому полные gates выполнены в WSL. Прежние CLI names/arguments/URL default/exit codes сохранены. Ограничения и примеры — в [CLI.md](CLI.md); исправление API — в [API.md](API.md).

Ветка подготовлена для отдельного PR в `main`; merge и deploy не входят в пакет. Исходные незакоммиченные работы сохранены в исходных checkout.

## Дополнительные gates перед PR

- Docs: 6 Python regression tests, `verify_docs.py --all`, `py_compile` и `bash -n` из актуального workflow; YAML workflow разобран parser-ом.
- Frontend: install с `--no-frozen-lockfile`, OpenAPI check и compatibility с `origin/main`, 32 files / 191 tests, lint и build — успешно. Отдельного format gate в workflow CI/CD нет.
- `pipeline wait` подтверждён subprocess assertions: чтение тела HTTP входит в deadline, чрезмерный poll interval ограничен оставшимся временем, timeout не отправляет cancel, terminal failure/cancellation дают прежний ненулевой код.
- Redaction проверяет фактический trimmed token, fallback общего token при пустом явном token и secret stdin с завершающим newline; отправляемое secret value сохраняет этот newline.
- Новый CI step создаёт `forge_test_cli` в существующем временном PostgreSQL service и переопределяет URL только для CLI tests. Existing integration suite продолжает использовать `forge_test_cicd`; contract regressions закрепляют разделение и feature-enabled Clippy.
- Changelog `[Unreleased]`, CLI/API/Data Model описывают итоговый diff; frontend и Services Base не изменены.

Проверка исходной справки CLI выявила вывод значения token env variable в `--help`. В итоговой ветке `hide_env_values` скрывает значение, сохраняя имя переменной; subprocess regression выполняется с заданным fixture token и проверяет stdout/stderr. После этого изменения повторены CLI tests, Clippy и release build CLI; API/backend fixtures не меняются.
