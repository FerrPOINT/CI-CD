# UI/UX Specification — Forge CI/CD

## 1. Общие принципы дизайна

- **Тема по умолчанию**: dark.
- **Цветовая палитра**: zinc-палитра для фона и поверхностей, indigo-акцент.
- **Типографика**: sans Inter / system-ui для UI, monospace для кода и логов.
- **Отступы**: 16px базовый grid.
- **Контрастность**: WCAG AA для текста.
- **Иконки**: lucide-react.
- **Локализация**: ru / en, LTR. Язык UI по умолчанию — русский.
- **Переход между темами**: переключатель в шапке, значение в `localStorage` ключ `theme`. Три темы: `dark`, `gray`, `light`.

### CSS-токены

Реализованы через `frontend/src/index.css` (Tailwind 4 `@theme` directive). Имена токенов совпадают с task-tracker для консистентности дизайн-системы:

| Токен | Dark (default) | Gray | Light |
|---|---|---|---|
| `--color-background` | `#09090b` | `#1f1f23` | `#f4f4f5` |
| `--color-surface` | `#18181b` | `#2a2a2f` | `#ffffff` |
| `--color-surface-raised` | `#27272a` | `#3f3f46` | `#f4f4f5` |
| `--color-border` | `#27272a` | `#3f3f46` | `#e4e4e7` |
| `--color-border-strong` | `#3f3f46` | `#52525b` | `#d4d4d8` |
| `--color-text-primary` | `#fafafa` | `#f4f4f5` | `#18181b` |
| `--color-text-secondary` | `#d4d4d8` | `#e4e4e7` | `#3f3f46` |
| `--color-text-muted` | `#a1a1aa` | `#a1a1aa` | `#71717a` |
| `--color-accent` | `#818cf8` | `#a5b4fc` | `#4f46e5` |
| `--color-accent-hover` | `#6366f1` | `#818cf8` | `#4338ca` |
| `--color-danger` | `#f87171` | `#fca5a5` | `#dc2626` |
| `--color-success` | `#4ade80` | `#86efac` | `#16a34a` |
| `--color-warning` | `#fbbf24` | `#fde047` | `#d97706` |

Tailwind utility classes: `bg-background`, `text-text-primary`, `bg-accent`, `border-border` и т.д.

### Статус-бейджи

| Статус | Цвет (dark) | Класс |
|---|---|---|
| `queued` | серо-зелёный | `.status-queued` |
| `running` | синий | `.status-running` |
| `success` | зелёный | `.status-success` |
| `failed` | красный | `.status-failed` |
| `canceled` | янтарный | `.status-canceled` |

---

## 2. Глобальный layout

```
+-----------------------------------------------------------+
| ☰ | Forge CI/CD |                    | Run pipeline | 👤   |
+-----------------------------------------------------------+
|                                                             |
|  [Sidebar]              [Main Content]                      |
|  - Dashboard            |                                   |
|  - Projects             |  ...                              |
|  - Pipelines            |                                   |
|  - Admin (если есть)    |                                   |
|                         |                                   |
+-----------------------------------------------------------+
```

### Top navigation (topbar)

- Логотип + название слева: «Forge CI/CD».
- Кнопка «Run pipeline» — справа, primary action.
- Theme toggle — переключение dark / gray / light.
- User avatar (Phase 1) → dropdown: profile, logout.

### Sidebar

- Collapsible, 285px по умолчанию.
- Section «Projects»: список проектов + кнопка «+ Add».
- Активный проект подсвечен accent-цветом.
- Каждый проект: имя + ветка по умолчанию.

### Адаптивность

- `≥ 801px`: двухколоночный grid (sidebar 285px + content 1fr).
- `< 800px`: одноколоночный, sidebar сверху, stages в одну колонку.

---

## 3. Login (Phase 1 — planned)

```
+-----------------------------------------------------------+
|                                                             |
|                    Forge CI/CD                              |
|                    Self-hosted control plane               |
|                                                             |
|                ┌─────────────────────────┐                 |
|                │                         │                 |
|                │    Email                │                 |
|                │  ┌─────────────────┐    │                 |
|                │  │                 │    │                 |
|                │  └─────────────────┘    │                 |
|                │                         │                 |
|                │    Password             │                 |
|                │  ┌─────────────────┐    │                 |
|                │  │                 │    │                 |
|                │  └─────────────────┘    │                 |
|                │                         │                 |
|                │  [  Sign in  ]          │                 |
|                │                         │                 |
|                └─────────────────────────┘                 |
|                                                             |
+-----------------------------------------------------------+
```

- Центрированная карточка на `--color-surface`.
- Поля: email, password.
- Кнопка «Sign in» — `--color-accent` background.
- Ошибка — alert с `--color-danger`.
- Нет регистрации в MVP — пользователи создаются через CLI/admin.

---

## 4. Dashboard

```
+-----------------------------------------------------------+
|  SELF-HOSTED CONTROL PLANE                  [Run pipeline] |
|  Forge CI/CD                                                |
+-----------------------------------------------------------+
|                                                             |
|  ┌──────────────┐  ┌──────────────────────────────────┐   |
|  │ Projects  +  │  │ Pipeline runs                     │   |
|  │              │  │ git@github.com:org/repo.git       │   |
|  │ ┌──────────┐ │  │                                    │   |
|  │ │my-service│ │  │ ┌────────────────────────────┐    │   |
|  │ │  main    │ │  │ │ #a1b2c3d4  main  [QUEUED]  │    │   |
|  │ └──────────┘ │  │ │            2026-08-26 10:05│    │   |
|  │ ┌──────────┐ │  │ └────────────────────────────┘    │   |
|  │ │api-gw    │ │  │ ┌────────────────────────────┐    │   |
|  │ │  main    │ │  │ │ #e5f6g7h8  main  [SUCCESS] │    │   |
|  │ └──────────┘ │  │ │            2026-08-25 18:30│    │   |
|  │              │  │ └────────────────────────────┘    │   |
|  └──────────────┘  └──────────────────────────────────┘   |
|                                                             |
|  ┌─────────────────────────────────────────────────────┐  |
|  │ PIPELINE #a1b2c3d4  main              [QUEUED]      │  |
|  │                                                       │  |
|  │  ┌──────────┐  ┌──────────┐  ┌──────────┐           │  |
|  │  │ BUILD    │  │ TEST     │  │ DEPLOY   │           │  |
|  │  │ [QUEUED] │  │ [QUEUED] │  │ [QUEUED] │           │  |
|  │  │──────────│  │──────────│  │──────────│           │  |
|  │  │ checkout │  │unit-tests│  │ deploy   │           │  |
|  │  │alpine/git│  │ rust:1.86│  │alpine:3. │           │  |
|  │  │git fetch │  │cargo test│  │echo deploy│          │  |
|  │  │ [QUEUED] │  │ [QUEUED] │  │ [QUEUED] │           │  |
|  │  │[Start]   │  │[Start]   │  │[Start]   │           │  |
|  │  │[Logs]    │  │[Logs]    │  │[Logs]    │           │  |
|  │  └──────────┘  └──────────┘  └──────────┘           │  |
|  │                                                       │  |
|  │  ┌─────────────────────────────────────────────┐     │  |
|  │  │ 001  Starting checkout...                   │     │  |
|  │  │ 002  Fetching remotes                       │     │  |
|  │  └─────────────────────────────────────────────┘     │  |
|  └─────────────────────────────────────────────────────┘  |
|                                                             |
+-----------------------------------------------------------+
```

### Элементы

- **Masthead** — заголовок «Forge CI/CD» + eyebrow «SELF-HOSTED CONTROL PLANE» + кнопка «Run pipeline».
- **Sidebar (Projects)** — список проектов, кнопка «+ Add» для формы создания.
- **Content (Pipeline runs)** — список последних запусков, каждый с short ID (`#a1b2c3d4`), git-реф, status badge, время.
- **Detail panel** — детали выбранного пайплайна: stages grid (3 колонки), jobs с actions, logs panel.
- **Actions** на job:
  - `queued`: `[Start]` → `running`, `[Logs]`
  - `running`: `[Pass]` → `success`, `[Fail]` → `failed`, `[Logs]`
  - terminal: `[Logs]` only
- **Logs panel** — `<pre>` с моноширинным текстом, формат `NNN  message`.

### Создание проекта

Форма появляется в sidebar при клике «+ Add»:

```
┌──────────────────────────┐
│ Project name             │
│ ┌──────────────────────┐ │
│ │                      │ │
│ └──────────────────────┘ │
│ git@github.com:org/repo │
│ ┌──────────────────────┐ │
│ │                      │ │
│ └──────────────────────┘ │
│ Branch                   │
│ ┌──────────────────────┐ │
│ │ main                 │ │
│ └──────────────────────┘ │
│ [ Create project ]       │
└──────────────────────────┘
```

---

## 5. Projects list (целевая страница)

```
+-----------------------------------------------------------+
|  Projects                              [+ Create project] |
+-----------------------------------------------------------+
|                                                             |
|  ┌──────────────────┐  ┌──────────────────┐               |
|  │ 📦 my-service    │  │ 📦 api-gateway   │               |
|  │ git@github.com..│  │ git@github.com..│               |
|  │ main             │  │ develop          │               |
|  │ 42 pipelines     │  │ 18 pipelines     │               |
|  │ Last: SUCCESS    │  │ Last: FAILED     │               |
|  └──────────────────┘  └──────────────────┘               |
|                                                             |
|  ┌──────────────────┐                                      |
|  │ 📦 web-frontend  │                                      |
|  │ git@github.com..│                                      |
|  │ main             │                                      |
|  │ 7 pipelines      │                                      |
|  │ Last: RUNNING    │                                      |
|  └──────────────────┘                                      |
|                                                             |
+-----------------------------------------------------------+
```

- Card grid: иконка, имя, repository_url, ветка, count pipelines, последний статус.
- Клик по карточке → переход к списку пайплайнов проекта.
- Empty state: «Add the first repository.» + кнопка создания.
- **Адаптив**: <768px — 1 колонка, 768–1279px — 2, ≥1280px — 3.

---

## 6. Pipeline list (целевая страница)

```
+-----------------------------------------------------------+
|  my-service / Pipelines               [+ Run pipeline]    |
|  git@github.com:org/my-service.git                         |
+-----------------------------------------------------------+
|                                                             |
|  ┌──────────────────────────────────────────────────────┐ |
|  │ #  │ Git ref   │ Status    │ Created    │ Duration   │ |
|  │────┼───────────┼───────────┼────────────┼────────────│ |
|  │ a1 │ main      │ [SUCCESS] │ 26.08 10:05│ 2m 15s     │ |
|  │ e5 │ main      │ [FAILED]  │ 25.08 18:30│ 5m 03s     │ |
|  │ f9 │ feature/x │ [RUNNING] │ 25.08 14:00│ —          │ |
|  │ 2c │ main      │ [CANCELED]│ 24.08 09:15│ —          │ |
|  └──────────────────────────────────────────────────────┘ |
|                                                             |
+-----------------------------------------------------------+
```

- Таблица с пайплайнами проекта.
- Колонки: short ID, git ref, status badge, created, duration.
- Клик по строке → переход к деталям пайплайна.
- Кнопка «Run pipeline» — trigger для default_branch.

---

## 7. Pipeline detail (stages / jobs / logs)

```
+-----------------------------------------------------------+
|  ← Pipelines | Pipeline #a1b2c3d4                          |
+-----------------------------------------------------------+
|                                                             |
|  PIPELINE #a1b2c3d4    main              [RUNNING]         |
|  Started: 2026-08-26 10:05                                 |
|                                                             |
|  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     |
|  │ BUILD        │  │ TEST         │  │ DEPLOY       │     |
|  │ [SUCCESS]    │  │ [RUNNING]    │  │ [QUEUED]     │     |
|  │──────────────│  │──────────────│  │──────────────│     |
|  │              │  │              │  │              │     |
|  │ checkout     │  │ unit-tests   │  │ deploy       │     |
|  │ alpine/git   │  │ rust:1.86    │  │ alpine:3.21  │     |
|  │ git fetch    │  │ cargo test   │  │ echo deploy  │     |
|  │ [SUCCESS]    │  │ [RUNNING]    │  │ [QUEUED]     │     |
|  │              │  │              │  │              │     |
|  │ [Logs]       │  │ [Pass][Fail] │  │ [Start]      │     |
|  │              │  │ [Logs]       │  │ [Logs]       │     |
|  └──────────────┘  └──────────────┘  └──────────────┘     |
|                                                             |
|  ┌─────────────────────────────────────────────────────┐  |
|  │ Logs: checkout                                      │  |
|  │─────────────────────────────────────────────────────│  |
|  │ 001  Cloning into /workspace...                    │  |
|  │ 002  Fetching origin                               │  |
|  │ 003  Checking out main                             │  |
|  │ 004  Build completed successfully                  │  |
|  └─────────────────────────────────────────────────────┘  |
|                                                             |
+-----------------------------------------------------------+
```

### Элементы

- **Breadcrumb** — «← Pipelines | Pipeline #<short-id>».
- **Pipeline header** — short ID, git_ref, status badge, started time.
- **Stages grid** — 3 колонки (≥800px), каждая стадия — карточка.
- **Stage card** — заголовок (имя + status badge), список jobs.
- **Job** — name, image (`code`), command (`code`), status badge, action buttons.
- **Action buttons** (зависят от статуса):
  - `queued`: `[Start]` (→ running), `[Logs]`
  - `running`: `[Pass]` (→ success), `[Fail]` (→ failed), `[Logs]`
  - `success` / `failed` / `canceled`: `[Logs]` only
- **Logs panel** — моноширинный `<pre>`, формат: `NNN  message`.
- **Адаптив**: <800px — stages в одну колонку.

### Status flow visualization

```
  [QUEUED] ──Start──→ [RUNNING] ──Pass──→ [SUCCESS] ✓
                         │
                         ├──Fail──→ [FAILED] ✗
                         │
                         └──Cancel──→ [CANCELED] ⊘
```

---

## 8. Admin (Phase 9 — planned)

```
+-----------------------------------------------------------+
|  Admin                                                     |
+-----------------------------------------------------------+
|                                                             |
|  [ Users ] [ Settings ] [ Audit Log ]                      |
|                                                             |
|  ┌──────────────────────────────────────────────────────┐ |
|  │ Username    │ Role    │ Status   │ Created  │ Actions│ |
|  │─────────────┼─────────┼──────────┼──────────┼────────│ |
|  │ admin       │ admin   │ active   │ 26.08    │ [Edit] │ |
|  │ ci-runner   │ runner  │ active   │ 26.08    │ [Edit] │ |
|  │ dev         │ user    │ inactive │ 27.08    │ [Edit] │ |
|  └──────────────────────────────────────────────────────┘ |
|                                                             |
|  [+ Create user ]                                          |
|                                                             |
+-----------------------------------------------------------+
```

- Tabs: Users, Settings, Audit Log.
- Users — таблица с управлением пользователями (Phase 1+).
- Settings — системные настройки (инстанс, runner config).
- Audit Log — журнал действий администратора.

---

## 9. Состояния (loading / empty / error)

### Loading

- Skeleton grid: 3 карточки stages с pulsing placeholder.
- Sidebar: 3 skeleton-строки проектов.

### Empty

- **Нет проектов**: «Add the first repository.» + кнопка создания.
- **Нет пайплайнов**: «No pipeline runs yet. Trigger one to create build, test and deploy jobs.»
- **Нет логов**: logs panel скрыт, кнопка «Logs» без действия.

### Error

- Alert с `--color-danger` текстом: `role="alert"`, сообщение об ошибке из API.
- Кнопка retry (целевое).

---

## 10. Frontend компоненты

### shadcn/ui (установлены)

| Компонент | Файл | Назначение |
|---|---|---|
| Button | `button.tsx` | Primary / secondary / danger actions |
| Card | `card.tsx` | Карточки проектов, stages |
| Input | `input.tsx` | Формы ввода |
| Label | `label.tsx` | Метки форм |
| Table | `table.tsx` | Таблицы (pipelines list, admin) |
| Tabs | `tabs.tsx` | Вкладки (admin, pipeline detail) |
| Dialog | `dialog.tsx` | Модальные окна (create project) |
| AlertDialog | `alert-dialog.tsx` | Подтверждения (cancel pipeline) |
| DropdownMenu | `dropdown-menu.tsx` | Меню (project actions, user menu) |
| Progress | `progress.tsx` | Индикатор выполнения |
| Textarea | `textarea.tsx` | Многострочный ввод |
| ThemeToggle | `theme-toggle.tsx` | Переключатель тем |

### Иконки (lucide-react)

- `GitBranch` — ветки
- `Play` — запуск пайплайна / start job
- `CheckCircle` — success
- `XCircle` — failed
- `Circle` — queued
- `Loader` / `Spinner` — running
- `Ban` — canceled
- `Terminal` — logs
- `Plus` — добавление
- `Settings` — настройки

---

## 11. Текущая реализация

Текущий frontend (`dashboard.tsx`) — single-page Dashboard, объединяющий:
- Список проектов (sidebar) + форма создания.
- Список пайплайнов выбранного проекта (content).
- Детали выбранного пайплайна (stages + jobs + logs).
- Управление статусами jobs (Start / Pass / Fail).
- Просмотр логов.

Layout: `.shell` (max-width 1450px) → `.workspace` (grid 285px + 1fr).

Целевая архитектура: разбить на отдельные страницы с `react-router` (Login, Dashboard, Projects, Pipeline list, Pipeline detail, Admin) по мере реализации фаз 1–9.

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/API.md` — REST API спецификация.
- `frontend/src/index.css` — CSS-токены.
- `frontend/src/styles.css` — текущие глобальные стили.
- `frontend/src/dashboard.tsx` — текущий Dashboard компонент.
