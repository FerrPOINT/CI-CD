# CLI Forge CI/CD

`cicd-cli` — утилита командной строки для управления control plane. Живёт в отдельном workspace-пакете `backend/cli` (ADR-0005) и общается с API исключительно по HTTP — не линкует серверный код.

## Сборка

```bash
# В Rust-контейнере (cargo на хосте отсутствует):
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo build -p cicd-cli'

./backend/target/debug/cicd-cli --help
```

## Конфигурация

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_API_URL` | `http://127.0.0.1:22801` | Базовый URL API |

## Команды

### project

```bash
cicd-cli project list
cicd-cli project create --name my-service \
  --repository-url http://127.0.0.1:22802/git/my-service.git \
  --branch main
```

### pipeline

```bash
cicd-cli pipeline list --project <PROJECT_UUID>
cicd-cli pipeline run --project <PROJECT_UUID> --ref main
cicd-cli pipeline show --id <PIPELINE_UUID>
```

### job

```bash
cicd-cli job start --job <JOB_UUID>   # queued → running
cicd-cli job pass  --job <JOB_UUID>   # → success
cicd-cli job fail  --job <JOB_UUID>   # → failed
cicd-cli job logs  --job <JOB_UUID>   # append-only логи
cicd-cli job log   --job <JOB_UUID> --message "custom line"
```

## Контракт

Группы команд и флаги зафиксированы тестом `backend/cli/tests/cli_contract.rs` (`project`, `pipeline`, `job` в `--help`). Изменение набора команд требует обновления теста и этого документа.

## Плановое (Phase C/D)

- Команды платформенных ресурсов: `runner`, `secret`, `artifact`, `environment`, `schedule`, `webhook`, `user`, `token`.
- Аутентификация: `--token` / `CICD_API_TOKEN` после включения auth middleware.
- Формат вывода `--json`/table, пагинация `--limit/--offset`.

## References

- `backend/cli/src/main.rs`
- `docs/API.md`
- `docs/adr/0005-workspace-layered-architecture.md`
