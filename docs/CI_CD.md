# CI/CD Forge CI/CD

## Назначение

Пока Forge CI/CD развивается, проект использует GitHub Actions для собственной непрерывной интеграции. Workflow расположен в `.github/workflows/ci.yml` и запускается на `push` и `pull_request` в ветку `main`. Это временный внешний контур доверия: продукт уже имеет embedded Docker/shell executor, но ещё не использует изолированный self-hosted runner pool с registration tokens, leases и sandboxing.

## Текущий workflow

Workflow состоит из трёх jobs:

| Job | Рабочая директория | Проверки |
|---|---|---|
| `backend` | `backend/` | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release` |
| `frontend` | `frontend/` | `pnpm install --frozen-lockfile`, `pnpm test`, `pnpm build` |
| `containers` | корень | `docker compose build` после успешных backend и frontend jobs |

Backend job использует Rust `1.86` с компонентами `rustfmt` и `clippy`, а frontend — Node.js `22` и pnpm `11`. Container job подтверждает, что Dockerfiles и `docker-compose.yml` собираются из чистого checkout.

## Локальное воспроизведение CI

Перед отправкой изменений воспроизводить релевантные проверки локально:

```bash
# Rust toolchain запускается внутри Docker.
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" \
  -w /workspace rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo fmt --check && /usr/local/cargo/bin/cargo clippy --all-targets -- -D warnings && /usr/local/cargo/bin/cargo test && /usr/local/cargo/bin/cargo build --release'

cd frontend
pnpm test
pnpm build
cd ..

docker compose build
```

`justfile` предоставляет короткие команды `just test-backend`, `just test-frontend` и `just build-frontend`. Для smoke-проверки запустить `docker compose up --build -d` и запросить `http://127.0.0.1:22801/api/v1/health`.

## Требования к изменению workflow

- Workflow обязан оставаться воспроизводимым из чистого checkout: зависимости frontend устанавливаются с `--frozen-lockfile`.
- Любая новая обязательная проверка должна быть описана в `README.md`, `docs/TESTING.md` и, при необходимости, в `AGENTS.md`.
- Проверки не должны требовать production secrets. Интеграции с закрытыми системами изолируются в защищённых workflows и запускаются только из доверенных событий.
- Docker build не публикует образы автоматически. Публикация по тегу принадлежит процессу из `docs/RELEASE.md`.
- Изменение API, миграций или контейнеров требует соответствующих contract/smoke-проверок.

## Переход к self-hosted runner

Целевое состояние — Forge CI/CD запускает собственный pipeline на self-hosted runner-е. До этого требуется реализовать runner registration, аутентификацию, lease/heartbeat, диспетчеризацию, sandboxing, потоковую доставку логов, безопасную работу с секретами и artifact storage.

Переход выполняется поэтапно:

1. Сохранить GitHub Actions как независимый контрольный workflow.
2. Добавить в Forge CI/CD отдельный non-blocking pipeline, который повторяет проверки backend, frontend и контейнеров.
3. Сопоставить результаты, длительность и логи двух контуров; проверить отказ runner-а и повторную доставку job.
4. Сделать self-hosted pipeline обязательным для выбранной ветки только после подтверждения безопасности и надёжности.

Самохостинг не отменяет необходимость внешней проверки: изменение control plane не должно единолично подтверждать собственную корректность.

## Связанные документы

- `.github/workflows/ci.yml`
- `docs/RELEASE.md`
- `docs/TESTING.md`
- `docs/RESILIENCE.md`