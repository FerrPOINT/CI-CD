# Проверка CLI CI/CD

Проверено 2026-10-02 в изолированном task checkout `feat/cli-workflows`.

## Среда и обязательные gates

Ubuntu WSL, rustc 1.88.0 (6b00bc388 2025-06-23), Node 22.23.3 / pnpm 10.28.1, Python 3.12.3. Чистый опубликованный Services Base `main`: `69bd8ef0fe424c2018bcdc509ddd25f7fce02a7e`. Политика зависимости от `main` сохраняется; Base и исходные dirty checkout не изменены этой задачей. Product lockfile обновлён под изменившиеся зависимости опубликованного Base без обновления registry versions.

Backend: fmt, workspace/all-target Clippy с `-D warnings`, workspace tests, OpenAPI drift и release workspace — успешно. Workspace: **161 passed, 0 ignored**, 0 failed. CI/CD дополнительно проверен штатным параллельным workspace invocation; Task Tracker и Wiki — последовательным invocation их workflows.

```bash
cd backend
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace -- --test-threads=1
cargo build --locked --release --workspace
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cicd cargo test --locked -p cicd-server --features integration --test integration_db -- --test-threads=1
CICD_TEST_DATABASE_URL=postgres://.../forge_test_cli cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings
```

Docs validators и существующие CI-contract tests проходят. Frontend: install с `--no-frozen-lockfile`, OpenAPI check/compat с `origin/main`, tests, lint и build — успешно. Task Tracker дополнительно typecheck; Task Tracker/Wiki — предусмотренный format check. Frontend tests: Task Tracker 253, CI/CD 191, Wiki 182.

## Регрессии и проверенные сценарии

- `download_transport`: обрыв Content-Length, timeout после headers, JSON/text errors, exit 3/TRANSPORT_ERROR/status null, redaction/request ID, сохранность destination с overwrite, отсутствие частичного файла, success и no-clobber. До исправления regression возвращал exit 2.
- `wait_deadline`: оба порядка HTTP/wait timeout и равные ограничения, задержка headers и тела, flag/env с приоритетом flag; exit 3 и правильный TRANSPORT_ERROR/WAIT_TIMEOUT, запросов retry/cancel нет. До исправления более короткий HTTP timeout игнорировался.
- Existing workflows дополнительно проверяют queued → running → success, failed/canceled с последним JSON, sleep в пределах deadline, variables, manual jobs, attempt logs, secrets stdin и repository/PR.
- PostgreSQL server integration suite и CLI real API выполнены раздельно: `forge_test_cicd` и `forge_test_cli`, PostgreSQL 17.6. Real CLI использует production HTTP handlers, временные Git/artifact directories; fixture задаёт execution states. Production runner/deploy не запускаются.

Существующие проверки file/stdin, pages, JSON/204, access/validation/conflict, transport timeout, credential redaction и download no-clobber сохраняются и проходят. Новые regressions воспроизвели замечания на исходной ветке, затем прошли после исправлений.

## Границы подтверждения

Проверки используют только fixture данные и собственные временные ресурсы. Постоянные Compose-группы, runtime images, volumes и production deployment не менялись. Windows native linking недоступен (`link.exe`); Rust gates выполнены в WSL. Новых endpoint, миграций или изменений Services Base нет. Merge и deploy не выполняются.

Описание команд, configuration, input/output/errors и ограничения: [CLI.md](CLI.md).
