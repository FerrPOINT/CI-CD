# Проверка CLI CI/CD

Проверено 2026-10-01 в отдельном task checkout `feat/cli-workflows`.

## Пройденные проверки

Среда: Ubuntu WSL, Rust 1.88.0, локальный PostgreSQL 16 в отдельном временном кластере с отдельной БД. Source snapshot скопирован из task checkout; sibling crates Services Base используются без изменений в рамках этой задачи. `Cargo.lock` синхронизирован с текущими зависимостями Base, включая уже существующую версию telemetry 0.33.

Из `backend`:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace -- --test-threads=1
CICD_TEST_DATABASE_URL=postgres://... cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings
cargo build --locked --release --workspace
```

- Workspace: 156 тестов, 0 failed. CLI: 3 unit, 10 contract, 7 subprocess/HTTP workflows.
- Real API/PostgreSQL: 2 теста, 0 failed; auth/RBAC/PAT/redaction и полный рабочий сценарий CLI.
- Фильтры, страницы, variables, PR description stdin, secret stdin, JSON stdout/empty success, структурированные stderr, parser credential redaction и download no-clobber проверены subprocess assertions.
- `pipeline wait`: polling queued→success, failed/canceled с ненулевым exit code, ограниченный deadline с медленным HTTP-ответом или чрезмерным poll interval, отсутствие cancel при timeout.
- Реальный API: repository create/refs/tree/blob/tags/commits/compare/delete, PR create/list/get/close/reopen/merge, pipeline variables/replay/cancel/retry/wait, manual job play и прежние start/fail, job retry/attempt logs, artifact download, protected environment и deployment approve/reject/rollback.
- Merge PR до создания связанного CI-проекта проверяет исправление NULL aggregate в protected branch lookup.

## Границы подтверждения

Real API запускает production handlers с настоящим PostgreSQL, отдельными repository/artifact directories и временными Git fixtures. Runner не запускается: manual flag и финальные execution/deployment statuses задаются тестовой fixture; лог/artifact создаются через HTTP. Это проверка управления и API-контрактов, а не исполнение production job или deploy. Остальные env-gated server integration suites не входят в количество 156 и целиком не запускались.

UI, Docker images и runtime окружения продуктов не изменялись. Windows native linking недоступен (`link.exe`), поэтому полные gates выполнены в WSL. Прежние CLI names/arguments/URL default/exit codes сохранены. Ограничения и примеры — в [CLI.md](CLI.md); исправление API — в [API.md](API.md).

Push, merge в рабочие ветки и deploy не выполнялись. Исходные незакоммиченные работы сохранены в исходных checkout.
