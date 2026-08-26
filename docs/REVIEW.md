# Review — Процесс код-ревью Forge CI/CD

## 1. Обзор

Единый процесс код-ревью для всех Pull Requests в репозиторий Forge CI/CD. Определяет: кто ревьюит, критерии approve, размер PR, тайминг, эскалацию.

> Детальный чек-лист review — в `docs/CODE_REVIEW.md`. Этот документ описывает процесс и роли; `CODE_REVIEW.md` — что именно проверять.

---

## 2. Кто ревьюит

### 2.1. Роли в review

| Роль | Кто | Права |
|------|-----|-------|
| **Автор** | Разработчик, создавший PR | Открывает PR, отвечает на комментарии, пушит исправления |
| **Reviewer** | Любой член команды, кроме автора | Проводит review, оставляет комментарии, approve / request changes |
| **Tech Lead** | Александр Жуков | Финальное решение при разногласиях, review архитектурных PR |

### 2.2. Назначение reviewer

- Автор назначает минимум **1 reviewer** на PR.
- Для PR, затрагивающих архитектуру (`docs/ARCHITECTURE.md`, ADR, `backend/src/domain.rs`, `backend/src/store.rs`) — **обязательно** Tech Lead.
- Для PR, затрагивающих security (`docs/SECURITY.md`, auth, secrets) — **обязательно** Tech Lead.
- Для PR «docs only» — достаточно 1 любого reviewer.

### 2.3. Self-review

- Автор проводит self-review перед назначением reviewer.
- Self-review обязателен: автор просматривает свой diff и проверяет чек-лист из `docs/CODE_REVIEW.md` раздел 3.

---

## 3. Критерии approve

### 3.1. Обязательные условия merge

PR может быть смержён только при выполнении **всех** условий:

1. **CI green** — все проверки GitHub Actions проходят:
   - `cargo fmt --check` — без изменений.
   - `cargo clippy -- -D warnings` — без предупреждений.
   - `cargo test` — все тесты green.
   - `cargo build --release` — без ошибок.
   - `pnpm build` — без ошибок.
   - `pnpm test` — все тесты green.
   - `npx tsc --noEmit` — без ошибок типов.
   - `docker compose build` — без ошибок.
   - Smoke test: `curl /api/v1/health` → 200 OK.

2. **Минимум 1 approval** — от назначенного reviewer.

3. **Все `[BLOCKING]` комментарии resolved** — автор и reviewer согласовали решения.

4. **Branch protection** — `main` защищён, linear history (rebase, no merge commits).

### 3.2. Критерии approve от reviewer

Reviewer approve PR, если:

- [ ] Код решает заявленную задачу (соответствует issue/PR description).
- [ ] Покрыты edge cases (пустые входные данные, max значения, невалидные переходы).
- [ ] Добавлены тесты для новой функциональности.
- [ ] Не сломаны существующие тесты.
- [ ] API-изменения отражены в `docs/API.md`.
- [ ] Изменения дата-модели отражены в `docs/DATA_MODEL.md`.
- [ ] Слои соблюдены: `api → domain → store`.
- [ ] Нет `unwrap()` / `expect()` в production коде.
- [ ] SQL только через parameterized queries.
- [ ] Нет секретов в diff.
- [ ] Компоненты на shadcn/ui + Tailwind (не кастомный CSS).
- [ ] Текстовые строки — через i18next (не хардкод).

> Полный чек-лист — `docs/CODE_REVIEW.md` раздел 3.

### 3.3. Комментарии review

| Префикс | Значение | Блокирует merge? |
|---------|----------|-------------------|
| `nit:` | Мелкое замечание (стиль, именование) | Нет |
| `question:` | Вопрос по коду | Нет (но автор должен ответить) |
| `issue:` | Проблема, требующая исправления | Да, если помечено `[BLOCKING]` |
| `[BLOCKING]` | Блокирующая проблема | Да |

---

## 4. Размер PR

### 4.1. Лимиты

| Тип PR | Максимальный размер | Примечание |
|--------|---------------------|------------|
| Фича / фикс | **400–500 строк** (не считая тесты и автосгенерированный код) | Один PR = одна логическая единица |
| Рефакторинг | **300 строк** | Отдельно от новой функциональности |
| Docs only | Без лимита | Документация не требует split |
| Автосгенерированный | Без лимита | `pnpm-lock.yaml`, `Cargo.lock`, миграции |

### 4.2. Когда разбивать PR

- PR > 500 строк → разбить на несколько.
- Смешивание рефакторинга и новой функциональности → разбить.
- Несколько несвязанных фич → разбить.
- Большой migration + код, использующий миграцию → два PR (миграция отдельно).

### 4.3. Красные флаги (auto-reject)

| Флаг | Действие |
|------|----------|
| PR > 500 строк (без тестов) | Request split |
| Секреты в коде | Request changes, уведомить автора |
| `unwrap()` в production коде | Request changes |
| SQL через `format!` | Request changes |
| CI red | Request fix |
| Слои нарушены (api → store напрямую) | Request changes |
| Несоблюдение conventional commits | Request rebase |

> См. `docs/CODE_REVIEW.md` раздел 6.

---

## 5. Тайминг

### 5.1. SLA review

| Этап | SLA | Ответственный |
|------|-----|---------------|
| Первый ответ на PR | **24 часа** (рабочие дни) | Reviewer |
| Ответ на комментарий автора | **24 часа** | Автор |
| Re-review после исправлений | **24 часа** | Reviewer |
| Финальный approve | **48 часов** от открытия PR | Reviewer |

### 5.2. Правила

- PR не висит без review > 24 часов (рабочие дни).
- Если reviewer недоступен > 24 часов — автор переназначает другого reviewer.
- PR с пометкой `[URGENT]` (hotfix, security) — SLA 4 часа.
- Выходные и праздники не входят в SLA.

### 5.3. Жизненный цикл PR

```
1. Автор открывает PR + self-review
   ↓
2. Назначает reviewer (минимум 1)
   ↓
3. Reviewer проводит review (SLA: 24h)
   ├── Approve → ждём CI green → merge
   ├── Request changes → автор исправляет → re-review (SLA: 24h)
   └── Комментарии → автор отвечает → resolve
   ↓
4. Все [BLOCKING] resolved + CI green + 1 approval
   ↓
5. Squash merge в main
   ↓
6. Удаление feature-ветки
```

---

## 6. Эскалация

### 6.1. Когда эскалировать

| Ситуация | Действие |
|----------|----------|
| PR без review > 48 часов | Эскалация Tech Lead |
| Разногласие автор ↔ reviewer | Эскалация Tech Lead |
| Архитектурное решение в PR | Обязательно Tech Lead review |
| Security-вопрос (секреты, auth, SQL injection) | Обязательно Tech Lead review |
| PR > 500 строк и автор отказывается разбивать | Эскалация Tech Lead |
| CI red > 24 часов | Эскалация Tech Lead |

### 6.2. Процесс эскалации

1. Автор или reviewer обращается к Tech Lead (Александр Жуков).
2. Tech Lead проводит review и принимает решение.
3. Решение Tech Lead — финальное (если нет аргументов для изменения решения).
4. При архитектурном решении — создаётся ADR (`docs/adr/`, см. `docs/ADR.md`).

### 6.3. Hotfix

- Для hotfix (production bug) — PR с пометкой `[URGENT]`.
- SLA review: 4 часа.
- Допускается merge с 1 approval (без полного review чек-листа, если риски обоснованы).
- Post-merge: отдельный PR с тестами и документацией в течение 48 часов.

---

## 7. Коммиты

### 7.1. Conventional commits

```
feat:     новая функциональность
fix:      исправление бага
docs:     документация
refactor: рефакторинг без изменения поведения
test:     тесты
chore:    обслуживание (зависимости, CI config)
perf:     производительность
```

### 7.2. Правила

- Один коммит = одна логическая единица.
- Сообщения коммитов на английском, понятные.
- Не `amend` / `squash` без явного запроса.
- Push только после локальной проверки: `cargo test`, `cargo clippy`, `pnpm test`, `pnpm build`.
- Linear history: rebase на `main`, не merge commits.

### 7.3. Squash merge

- При merge в `main` — squash merge (один коммит в `main`).
- Feature-ветка удаляется после merge.

> См. `docs/AGENTS.md` раздел «Коммиты и документация».

---

## 8. Branch protection

- `main` защищён.
- Требуется **1 approval**.
- Требуется **CI green** (все проверки).
- **Require linear history** (rebase, no merge commits).
- Разрешён **squash merge**.
- Запрещён **force push** в `main`.

> См. `docs/CODE_REVIEW.md` раздел 5.2.

---

## 9. Quick reference (для автора PR)

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

# Открыть PR, назначить reviewer, ждать review (SLA: 24h)
```

---

## 10. References

- `docs/CODE_REVIEW.md` — детальный чек-лист review.
- `docs/CODE_STYLE.md` — конвенции кода.
- `docs/TESTING.md` — стратегия тестирования.
- `docs/ARCHITECTURE.md` — слои приложения.
- `docs/AGENTS.md` — правила работы в репозитории.
- `docs/ADR.md` — создание архитектурных решений.