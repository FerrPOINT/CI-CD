# CLI Forge CI/CD

`cicd-cli` - HTTP-only утилита управления control plane. Бинарь живёт в отдельном workspace-пакете `backend/cli` (ADR-0005), общается с API через публичные routes и не линкует серверный код, SQLx, Git storage или runner implementation.

## Сборка

```bash
# В Rust-контейнере, если cargo на хосте отсутствует:
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo build -p cicd-cli'

./backend/target/debug/cicd-cli --help
```

## Конфигурация

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_API_URL` | `http://127.0.0.1:22801` | Базовый URL API |
| `CICD_API_TOKEN` | - | Bearer PAT/JWT для auth-mode; эквивалентно `--token` |
| `CICD_OUTPUT` | `json` | Формат вывода: `json` или `table`; эквивалентно `--output` |

Глобальные флаги:

```bash
cicd-cli --api-url http://127.0.0.1:22801 --token "$CICD_API_TOKEN" --output table project list
```

## Команды

### project

```bash
cicd-cli project list --limit 50 --offset 0
cicd-cli project create --name my-service \
  --repository-url http://127.0.0.1:22802/git/my-service.git \
  --branch main
```

### pipeline

```bash
cicd-cli pipeline list --project <PROJECT_UUID> --limit 50 --offset 0
cicd-cli pipeline run --project <PROJECT_UUID> --git-ref main
cicd-cli pipeline run --project <PROJECT_UUID> --git-ref main --idempotency-key <UUID>
cicd-cli pipeline show --id <PIPELINE_UUID>
```

`pipeline run` автоматически создаёт UUID `Idempotency-Key`, если `--idempotency-key` не задан. Явный key нужен внешней automation, которая безопасно повторяет тот же запуск после transport error.

### job

```bash
cicd-cli job start --id <JOB_UUID>   # queued -> running
cicd-cli job pass  --id <JOB_UUID>   # -> success
cicd-cli job fail  --id <JOB_UUID>   # -> failed
cicd-cli job attempts --id <JOB_UUID>
cicd-cli job logs  --id <JOB_UUID>
cicd-cli job logs  --id <JOB_UUID> --attempt <ATTEMPT_UUID>
cicd-cli job log   --id <JOB_UUID> --message "custom line"
```

### runner

```bash
cicd-cli runner list
cicd-cli runner register --name shell-01 --tag shell --tag linux
cicd-cli runner heartbeat --id <RUNNER_UUID> --status online
cicd-cli runner delete --id <RUNNER_UUID>
```

### secret

```bash
cicd-cli secret list --project <PROJECT_UUID>
cicd-cli secret set --project <PROJECT_UUID> --key NPM_TOKEN --value "$NPM_TOKEN"
cicd-cli secret delete --id <SECRET_UUID>
```

Не передавайте production secret values через shared shell history. Для локального MVP допустим env-var input, для production target нужны protected token/secret input flows.

### artifact

```bash
cicd-cli artifact list --job <JOB_UUID>
cicd-cli artifact upload --job <JOB_UUID> --file ./build.zip --name build.zip --content-type application/zip
cicd-cli artifact download --id <ARTIFACT_UUID> --output ./build.zip
```

### environment / deployment

```bash
cicd-cli environment list --project <PROJECT_UUID>
cicd-cli environment create --project <PROJECT_UUID> --name staging --url https://staging.example.com
cicd-cli environment update --id <ENV_UUID> --status degraded
cicd-cli environment delete --id <ENV_UUID>

cicd-cli deployment list --environment <ENV_UUID>
cicd-cli deployment create --environment <ENV_UUID> --git-ref main --pipeline <PIPELINE_UUID> --status success
```

### schedule

```bash
cicd-cli schedule list --project <PROJECT_UUID>
cicd-cli schedule create --project <PROJECT_UUID> --cron "*/15 * * * *" --git-ref main --enabled true
cicd-cli schedule update --id <SCHEDULE_UUID> --cron "0 2 * * *" --git-ref main --enabled false
cicd-cli schedule delete --id <SCHEDULE_UUID>
```

### webhook / outbox / notification

```bash
cicd-cli webhook list --project <PROJECT_UUID>
cicd-cli webhook create --project <PROJECT_UUID> --url https://example.com/hook \
  --event pipeline.finished --secret "$WEBHOOK_SECRET"
cicd-cli webhook delete --id <WEBHOOK_UUID>

cicd-cli outbox list --project <PROJECT_UUID> --limit 50 --status failed --channel webhook
cicd-cli outbox show --id <DELIVERY_UUID>
cicd-cli outbox requeue --id <DELIVERY_UUID>

cicd-cli notification list --project <PROJECT_UUID>
cicd-cli notification replace --project <PROJECT_UUID> --config in_app=dashboard
cicd-cli notification events --project <PROJECT_UUID> --limit 50
```

`notification replace` заменяет весь набор notification configs проекта. Запуск без `--config` очищает список.

### report / audit

```bash
cicd-cli report summary --project <PROJECT_UUID>
cicd-cli audit list
```

### user / member / token

```bash
cicd-cli user list
cicd-cli user create --username alice --role developer --enabled true --password "$PASSWORD"
cicd-cli user update --id <USER_UUID> --username alice --role maintainer --enabled true

cicd-cli member list --project <PROJECT_UUID>
cicd-cli member upsert --project <PROJECT_UUID> --user <USER_UUID> --role developer
cicd-cli member remove --project <PROJECT_UUID> --user <USER_UUID>

cicd-cli token list
cicd-cli token create --name deploy-bot --user <USER_UUID> --project <PROJECT_UUID> \
  --scope api:read --scope api:write --expires-in-days 30
cicd-cli token revoke --id <TOKEN_UUID>
```

## Контракт

Группы команд и флаги зафиксированы тестом `backend/cli/tests/cli_contract.rs`: control-plane groups, `--token`, `--output`, pagination flags, job attempts/logs, `--idempotency-key` и ключевые platform mutations. Изменение публичного CLI surface требует обновления теста и этого документа.

## Границы MVP

- CLI использует только публичный HTTP API и повторяет его текущие ограничения auth/RBAC, pagination и validation.
- Stable JSON есть сейчас; `table` является lightweight TSV-like представлением для ручной работы, не production reporting format.
- Profiles, OS keyring, shell completion, request tracing headers, timeout policy, YAML/NDJSON и full real-API CLI integration gate остаются target из `docs/DELIVERY_ARCHITECTURE.md`.
- External email/Slack adapters и inbound provider webhooks по-прежнему target: CLI управляет текущими local notification/outbox/webhook MVP routes.

## References

- `backend/cli/src/main.rs`
- `backend/cli/tests/cli_contract.rs`
- `docs/API.md`
- `docs/adr/0005-workspace-layered-architecture.md`
