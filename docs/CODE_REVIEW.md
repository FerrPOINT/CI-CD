# Code Review — чек-лист для Pull Requests Forge CI/CD

## 1. Обзор

Единые правила review для всех Pull Requests в репозиторий Forge CI/CD. Цель — поддерживать качество кода, безопасность и консистентность при автоматическом и ручном review.

---

## 2. Обязательные проверки CI

PR не может быть смержён, пока все проверки не пройдены.

### 2.1. Backend (Rust)

| Проверка | Команда | Условие |
|---|---|---|
| Форматирование | `cargo fmt --check` | Без изменений |
| Linter | `cargo clippy -- -D warnings` | Без предупреждений |
| Тесты | `cargo test` | Все тесты green |
| Сборка | `cargo build --release` | Без ошибок |

### 2.2. Frontend (TypeScript)

| Проверка | Команда | Условие |
|---|---|---|
| Сборка | `pnpm build` | Без ошибок |
| Тесты | `pnpm test` | Все тесты green |
| Типы | `tsc --noEmit` | Без ошибок типов |

### 2.3. Docker

| Проверка | Команда | Условие |
|---|---|---|
| Сборка образов | `docker compose build` | Без ошибок |
| Smoke test | `docker compose up -d` + `curl /api/v1/health` | 200 OK |

---

## 3. Чек-лист review

### 3.1. Функциональность

- [ ] Код решает заявленную задачу (соответствует issue/PR description).
- [ ] Покрыты edge cases (пустые входные данные, max значения, невалидные переходы).
- [ ] Добавлены тесты для новой функциональности.
- [ ] Не сломаны существующие тесты.
- [ ] API-изменения отражены в `docs/API.md`.
- [ ] Изменения дата-модели отражены в `docs/DATA_MODEL.md`.

### 3.2. Качество кода

- [ ] `cargo fmt --check` — без изменений.
- [ ] `cargo clippy -- -D warnings` — чисто.
- [ ] `cargo test` — green.
- [ ] Нет `unwrap()` / `expect()` в production коде (только в тестах).
- [ ] Нет `clone()` там, где можно использовать borrowing (`&T`, `&mut T`).
- [ ] Error handling через `thiserror` (domain errors) / `anyhow` (application errors).
- [ ] Слои соблюдены: `api → domain → store`. HTTP-хендлеры не обращаются к БД напрямую.

### 3.3. Frontend

- [ ] `pnpm build` — без ошибок.
- [ ] `pnpm test` — green.
- [ ] `tsc --noEmit` — без ошибок типов.
- [ ] Компоненты на shadcn/ui + Tailwind CSS (не кастомный CSS).
- [ ] Серверное состояние — `@tanstack/react-query`, клиентское — `useState`/`zustand`.
- [ ] Текстовые строки — через i18next (не хардкод).
- [ ] Добавлены unit-тесты для новых компонентов.

### 3.4. Безопасность

- [ ] **Нет секретов в коде** — токены, пароли, ключи не закоммичены.
- [ ] **Нет секретов в diff** — проверить добавленные файлы на чувствительные данные.
- [ ] SQL только через parameterized queries (`sqlx::query` с `$1`, `$2`, ...).
- [ ] Нет `format!` для построения SQL-запросов.
- [ ] User input валидируется перед использованием.
- [ ] Нет `eval()` / `Function()` в frontend.
- [ ] Новые env-переменные добавлены в `.env.example` (без значений).
- [ ] Новые env-переменные имеют префикс `CICD_`.

### 3.5. Коммиты

- [ ] Conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`.
- [ ] Один коммит = одна логическая единица.
- [ ] Нет merge commits от `main` (использовать rebase).
- [ ] Сообщения коммитов на английском, понятные.
- [ ] Нет коммитов с `WIP`, `tmp`, `fix typo` в финальном PR (squash перед merge).

### 3.6. Размер

- [ ] **Максимальный размер PR: 400–500 строк** (не считая тесты и автосгенерированный код).
- [ ] Если PR больше — разбить на несколько.
- [ ] Один PR = одна фича/фикс (не смешивать рефакторинг и новую функциональность).

### 3.7. Документация

- [ ] Изменения API → обновлён `docs/API.md`.
- [ ] Изменения дата-модели → обновлён `docs/DATA_MODEL.md`.
- [ ] Новый функционал → обновлён `docs/ROADMAP.md`.
- [ ] Неочевидные решения → зафиксированы в `docs/ARCHITECTURE.md`.
- [ ] Новый документ → добавлен в индекс (README или docs index).

### 3.8. Тестирование

- [ ] Unit-тесты для domain-логики (transitions, aggregation).
- [ ] Integration-тесты для новых endpoints (API contract).
- [ ] CLI-тесты для новых команд.
- [ ] Frontend unit-тесты для новых компонентов.
- [ ] curl-проверка для новых endpoints (команда в PR description).

---

## 4. Процесс review

### 4.1. Перед review

1. Автор проверяет все пункты чек-листа локально.
2. CI green (все проверки проходят).
3. PR description содержит: что изменено, почему, как протестировано.
4. Скриншоты UI (если есть UI-изменения) — 375 / 1920 / 2560 px.

### 4.2. Во время review

1. Reviewer проверяет чек-лист.
2. Комментарии: `nit:` (мелочи), `question:` (вопросы), `issue:` (проблемы).
3. Блокирующие комментарии помечаются `[BLOCKING]`.
4. Автор отвечает на комментарии и пушит исправления.

### 4.3. После review

1. Все `[BLOCKING]` комментарии resolved.
2. CI green после последних изменений.
3. Squash merge (один коммит в `main`).
4. Удаление feature-ветки после merge.

---

## 5. Автоматизация (CI)

### 5.1. GitHub Actions workflow

```yaml
# .github/workflows/ci.yml
name: CI

on: [pull_request]

jobs:
  backend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test
      - run: cargo build --release

  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - run: pnpm install
      - run: pnpm build
      - run: pnpm test
      - run: npx tsc --noEmit

  docker:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: docker compose build
      - run: docker compose up -d
      - run: sleep 5 && curl -fsS http://127.0.0.1:22801/api/v1/health
```

### 5.2. Branch protection

- `main` защищён.
- Требуется 1 approval.
- Требуется CI green.
- Require linear history (rebase, no merge commits).
- Разрешён squash merge.

---

## 6. Красные флаги (auto-reject)

| Флаг | Действие |
|---|---|
| Секреты в коде | Request changes, уведомить автора |
| `unwrap()` в production коде | Request changes |
| SQL через `format!` | Request changes |
| PR > 500 строк (без тестов) | Request split |
| CI red | Request fix |
| Слои нарушены (api → store напрямую) | Request changes |
| Несоблюдение conventional commits | Request rebase |

---

## 7. Quick reference (для автора PR)

```bash
# Перед push — локальная проверка
cd /opt/dev/CI-CD

# Backend
cd backend && cargo fmt && cargo clippy -- -D warnings && cargo test && cd ..

# Frontend
cd frontend && pnpm build && pnpm test && npx tsc --noEmit && cd ..

# Docker smoke
docker compose up --build -d
curl -fsS http://127.0.0.1:22801/api/v1/health
docker compose down

# Коммит
git add -A
git commit -m "feat: add artifact download endpoint"
git push origin feature/artifact-download
```

---

## References

- `docs/CODE_STYLE.md` — конвенции кода
- `docs/TESTING.md` — стратегия тестирования
- `docs/ARCHITECTURE.md` — слои приложения
- `docs/AGENTS.md` — правила работы в репозитории