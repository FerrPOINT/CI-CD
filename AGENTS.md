# AGENTS.md — Forge CI/CD

## Репозиторий

- **Путь**: `/opt/dev/CI-CD`.
- **GitHub**: `git@github.com:FerrPOINT/CI-CD.git`.
- **Назначение**: self-hosted CI/CD control plane. В MVP jobs выполняет embedded runner (Docker/shell) с `job_queue` + `job_leases` ledger/reconciliation; внешний runner protocol MVP покрывает register/heartbeat/poll/ack/renew/logs/complete и basic runner tag matching, а `forge-runner` даёт отдельный shell-runner process поверх этого protocol. Production-grade dispatch policy, long-poll/wakeup, protocol secrets/artifacts, richer log chunks и sandbox hardening остаются target.
- **Стек**: Rust 2024, Axum 0.8, SQLx 0.8, PostgreSQL 17; React 19, Vite 6, Tailwind CSS 4, shadcn/ui.
- **Env prefix**: `CICD_`.
- **Порты**: API `22801`, Dashboard `22802`, PostgreSQL `22543`.

## Правила работы с репозиторием

1. Работать из `/opt/dev/CI-CD`; перед изменениями читать `docs/ARCHITECTURE.md`, `docs/DATA_MODEL.md`, `docs/API.md` и релевантный ADR.
2. До начала проверять `git status`. Не отменять и не перезаписывать чужие изменения; при неожиданном изменении во время работы остановиться и запросить указания.
3. Backend сохраняет слои `api -> domain -> store`. `JobStatus::transition_to()` — единственный источник правил перехода статусов. SQL-запросы параметризуются через SQLx.
4. Frontend использует React, Tailwind и shadcn/ui; новые API-клиенты и DTO остаются типизированными. Vite dev proxy направляет `/api` на `http://localhost:22801`.
5. Все секреты передаются только через `CICD_` переменные или secret manager. Не коммитить `.env`, токены, пароли и реальные данные.

## Коммиты и документация

- Использовать Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`.
- Один коммит — одна логическая единица. Не выполнять amend/squash или push без явного запроса пользователя.
- При изменении API обновлять `docs/API.md`; при изменении схемы — `docs/DATA_MODEL.md`; архитектурное решение фиксировать новым ADR в `docs/adr/`.
- Изменения Git Smart HTTP, bare repository storage или post-receive hook синхронизировать с `docs/GIT_HOSTING.md`.
- Документация проекта и пользовательские строки — на русском; код и комментарии — на английском согласно `docs/CODE_STYLE.md`.

## Docker и команды проверки

```bash
# Запуск и управление локальным окружением.
cp .env.example .env
docker compose up --build -d
docker compose ps
docker compose logs -f
docker compose down

# Backend: запуск через Docker, если cargo нет на хосте.
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" \
  -w /workspace rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test'

# Frontend.
cd frontend && pnpm test
cd frontend && pnpm build
```

Для полного backend gate использовать также `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` и `cargo build --release` в том же Rust-контейнере. `justfile` дублирует основные операции: `just up`, `just down`, `just test-backend`, `just test-frontend`, `just build-frontend`, `just health`.

Не использовать `docker compose restart` для применения нового образа или env: пересоздавать сервисы через `docker compose up -d` либо `docker compose up --build -d`.

## Checklist перед завершением

- [ ] Изменения соответствуют существующей архитектуре и не затёрли чужую работу.
- [ ] Backend-проверки проходят: fmt, clippy, test, release build.
- [ ] Frontend-проверки проходят: `pnpm test`, `pnpm build`.
- [ ] `docker compose config` и `docker compose build` успешны при изменении контейнеров.
- [ ] Для новых endpoint выполнена curl-проверка; для UI-изменений сделаны актуальные screenshots при необходимости.
- [ ] Документация, `.env.example` и ADR обновлены, если изменились контракт, конфигурация или решение.
- [ ] В diff отсутствуют credentials, токены, `.env` и другие чувствительные данные.

## References

- `docs/ARCHITECTURE.md`
- `docs/CODE_STYLE.md`
- `docs/TESTING.md`
- `docs/API.md`
- `docs/DATA_MODEL.md`
- `docs/CI_CD.md`
