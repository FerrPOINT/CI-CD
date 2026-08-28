# Итоговый план действий после аудита функционала/доков/скринов

Дата: 2026-08-26. Основание: автоматическая сверка исходников ↔ docs ↔ screenshots.

**Цель:** закрыть все найденные пробелы документации и скриншотов; привести их к единому стандарту task-tracker.

---

## Группа 1 — API.md: канонический контракт (приоритет 1)

API.md не содержит ~15 реализованных endpoint-ов. Добавить секции:

1.1. **Jobs/Pipelines actions:**
- `POST /api/v1/jobs/{job_id}/retry`
- `POST /api/v1/pipelines/{pipeline_id}/cancel`
- `POST /api/v1/pipelines/{pipeline_id}/retry`

1.2. **Internal:**
- `POST /api/v1/internal/git-push` (auth: `CICD_GIT_INTERNAL_TOKEN`; вызывается post-receive hook)

1.3. **Git/repo/PR-контур** (полные таблицы параметров/ответов по образцу существующих секций; детали поведения сослать на GIT_HOSTING.md / PULL_REQUESTS.md):
- `GET/POST /api/v1/repositories`, `DELETE /api/v1/repositories/{name}`
- `GET /api/v1/repos/{repo}/refs`
- `GET /api/v1/repos/{repo}/commits` (`?branch=&limit=`)
- `GET /api/v1/repos/{repo}/compare?from=&to=`
- `GET /api/v1/repos/{repo}/pulls`, `POST /api/v1/repos/{repo}/pulls`
- `POST /api/v1/repos/{repo}/pulls/{number}/action` (merge/close/reopen)
- `GET /git/{repo}/info/refs`, `POST /git/{repo}/git-upload-pack`, `POST /git/{repo}/git-receive-pack` (Smart HTTP, auth по `CICD_GIT_TOKEN`)

Проверка: скрипт-сверка «роуты из api.rs/platform.rs/git_host.rs/pulls.rs ⊆ API.md» → 0 пропусков.

## Группа 2 — Sync stale-доков (приоритет 2)

Заменить «planned/не реализовано» на фактический MVP-статус с honest TODO:

| Док | Что чинить |
|---|---|
| SYSTEM_ADMIN.md | users/audit/reports реализованы; убрать несуществующие `/admin/*` пути, указать реальные `/users`, `/audit-log`, `/projects/{id}/reports` |
| PROJECT_ADMIN.md | `GET/PATCH/DELETE /projects/{id}` реализованы; убрать «Phase 2» |
| GLOSSARY.md | runners/artifacts/webhooks/secrets — актуальные статусы |
| STORAGE.md | артефакты реализованы (CICD_ARTIFACTS_DIR, volume, 50 MiB) |
| DATABASE_INDEXES.md | 9 индексов создаются store.rs — убрать «нет индексов» |
| DEPLOYMENT.md / RESILIENCE.md / CI_CD.md | embedded runner реален; «runners postponed» → фактический статус + ссылка на RUNNER_ARCHITECTURE.md |
| EVENTS.md | статус SSE: не реализован (честно), ссылка на AUTOMATION_ARCHITECTURE.md |
| FRONTEND_ARCHITECTURE.md / UI_UX.md | 20 страниц/21 роут фактически; admin/settings — статические заглушки (честно пометить) |

Проверка: `rg -i 'planned|не реализован' docs/` → только легитимные target-маркеры.

## Группа 3 — Скриншоты (приоритет 3)

3.1. Переснять 01–12 в 1920×1080 (сейчас 1440×900) — единообразие с 13–21.
3.2. Добавить mobile 375×812: dashboard, projects, pipeline-detail, runners (таблица).
3.3. (опция) 2K 2560×1440: dashboard — по стандарту TESTING.md.
3.4. README: добавить секцию «Mobile» с новыми скринами (формат: `### Тайтл` + скрин).

Проверка: md5 уникальны; vision-проверка данных; размеры соответствуют.

## Группа 4 — Мелочи (приоритет 4, опционально)

4.1. README «Документы»: добавить ARCHITECTURE_INDEX в начало списка (уже есть — проверить).
4.2. `docs/ADR.md` секция 4 «Creating New ADRs» — пример номера «0005, 0006» устарел → «0009, 0010».
4.3. CI_CD.md / TESTING.md — зафиксировать фактический тест-станок (docker cargo gate, 17 backend / 2 frontend тестов) vs целевой (real-DB, Playwright) по DELIVERY_ARCHITECTURE.md.

## Порядок исполнения

Группа 1 → 2 → 3 → 4. Каждая группа = отдельный commit (`docs: api reference completion`, `docs: sync stale docs to actual mvp state`, `docs: unified screenshots + mobile`). Пуш после каждой группы или в конце — по решению владельца.

## Что НЕ входит (осознанно)

- Реализация auth/RBAC/outbox/runner-protocol — это код, не доки; порядок в `docs/ROADMAP.md` + `plans/architecture-rebuild-plan.md`.
- OpenAPI-генерация, mobile UX-переделка — Phase C/D целевой архитектуры.
