# Libraries — утверждённые зависимости Forge CI/CD

## 1. Обзор

Список одобренных библиотек для backend (Rust) и frontend (TypeScript/React). Описывает версию, назначение и политику добавления новых зависимостей.

---

## 2. Backend (Rust)

### 2.1. Утверждённые crates

| Crate | Версия | Назначение | Источник |
|---|---|---|---|
| `axum` | 0.8 | Web framework, HTTP routes, middleware | crates.io |
| `sqlx` | 0.8 | Async PostgreSQL driver, compile-time SQL checks | crates.io |
| `tokio` | 1 | Async runtime, task scheduling | crates.io |
| `serde` | 1 | Serialization/deserialization framework | crates.io |
| `serde_json` | 1 | JSON serialization (serde backend) | crates.io |
| `uuid` | 1 | UUID v4 generation and parsing | crates.io |
| `chrono` | 0.4 | Date/time types with timezone support | crates.io |
| `thiserror` | 2 | Ergonomic custom error types (domain errors) | crates.io |
| `clap` | 4 | CLI argument parsing with derive macros | crates.io |
| `reqwest` | 0.12 | HTTP client (CLI, webhook delivery, SMS API) | crates.io |
| `tower-http` | 0.6 | HTTP middleware (CORS, trace, compression) | crates.io |
| `tower` | 0.5 | Service abstraction, test utilities | crates.io |
| `tracing` | 0.1 | Structured logging, spans | crates.io |
| `tracing-subscriber` | 0.3 | Log formatting, env filter (RUST_LOG) | crates.io |
| `anyhow` | 1 | Application-level error handling | crates.io |
| `sha2` | 0.10 | SHA-256 checksums (artifacts, webhooks) | crates.io |
| `hmac` | 0.12 | HMAC-SHA256 (webhook signatures) | crates.io |
| `aes-gcm` | 0.10 | AES-256-GCM encryption (secrets) | crates.io |
| `base64` | 0.22 | Base64 encoding (encryption, checksums) | crates.io |

### 2.2. Dev-dependencies

| Crate | Версия | Назначение |
|---|---|---|
| `tower` | 0.5 | `TestServer` для API integration tests |

### 2.3. Особенности использования

| Crate | Правила |
|---|---|
| `sqlx` | Только parameterized queries (`$1`, `$2`). Не использовать `format!` для SQL. |
| `tokio` | Runtime через `#[tokio::main]` в `main.rs`. Не создавать дополнительные runtimes. |
| `serde` | `#[derive(Serialize, Deserialize)]` для всех DTO. `#[serde(rename_all = "camelCase")]` для JSON API. |
| `uuid` | Только `Uuid::new_v4()`. Не использовать v1/v5/v7 в MVP. |
| `chrono` | `DateTime<Utc>` для всех timestamp'ов. Не использовать `NaiveDateTime`. |
| `thiserror` | Для domain errors (типизированные ошибки). `anyhow` — для application errors (не типизированные). |
| `clap` | Derive API (`#[derive(Parser)]`). Subcommands с `#[command(subcommand)]`. |
| `reqwest` | Timeout 10s для webhook delivery, 30s для CLI requests. |

---

## 3. Frontend (TypeScript / React)

### 3.1. Утверждённые пакеты

| Пакет | Версия | Назначение |
|---|---|---|
| `react` | 19 | UI framework |
| `react-dom` | 19 | React DOM renderer |
| `react-router` | 7 | Client-side routing |
| `@tanstack/react-query` | 5 | Server state management (data fetching, caching) |
| `zustand` | 5 | Client state management (lightweight stores) |
| `i18next` | 24 | Internationalization |
| `react-i18next` | 15 | React bindings for i18next |
| `tailwindcss` | 4 | Utility-first CSS framework |
| `shadcn/ui` | — | Headless UI component library (Radix-based) |
| `lucide-react` | 0.4xx | Icon library |
| `sonner` | 1 | Toast notifications |
| `typescript` | 5.x | Type checking |
| `vite` | 6 | Build tool, dev server |
| `@vitejs/plugin-react` | — | React plugin for Vite |
| `vitest` | 3.x | Unit test framework |
| `@testing-library/react` | 16.x | Component testing utilities |
| `@testing-library/jest-dom` | 6.x | DOM assertion matchers |
| `recharts` | 2.x | Charts (reports, Phase 9) |
| `pnpm` | 11 | Package manager |

### 3.2. Особенности использования

| Пакет | Правила |
|---|---|
| `react` | Functional components + hooks. Без class components. |
| `react-router` | Data router API (v7). Lazy load через `lazy()`. |
| `@tanstack/react-query` | `useQuery` для GET, `useMutation` для POST/PATCH/DELETE. Query keys: `['pipelines', projectId]`. |
| `zustand` | Только для клиентского состояния (UI state, filters). Не для серверных данных. |
| `i18next` | Все тексты через `t('key')`. Не хардкодить строки в компонентах. |
| `tailwindcss` | Utility classes в JSX. Без кастомного CSS (только `@theme` в `index.css`). |
| `shadcn/ui` | Компоненты копируются в `src/components/ui/` (не npm-зависимость). Кастомизация через Tailwind. |
| `lucide-react` | Импорт по имени: `import { Bell } from 'lucide-react'`. Tree-shaking автоматически. |
| `sonner` | `<Toaster />` в root layout. `toast.success()`, `toast.error()`, `toast.loading()`. |

---

## 4. Политика версий

### 4.1. Семантическое версионирование

Все зависимости следуют SemVer: `MAJOR.MINOR.PATCH`.

| Изменение | Политика |
|---|---|
| PATCH (0.x.1 → 0.x.2) | Автоматически, безопасно |
| MINOR (0.1.x → 0.2.x) | Проверить changelog, обновить в отдельном PR |
| MAJOR (1.x → 2.x) | Обязательное review, отдельный PR, полный регресс |

### 4.2. Фиксация версий

| Файл | Формат | Пример |
|---|---|---|
| `Cargo.toml` | `version = "0.8"` | `axum = "0.8"` |
| `package.json` | `^x.y.z` | `"react": "^19.0.0"` |

### 4.3. Обновление

| Инструмент | Команда | Частота |
|---|---|---|
| Rust | `cargo update` | Раз в месяц |
| Frontend | `pnpm update` | Раз в месяц |
| Dependabot | Автоматические PR | Еженедельно |

### 4.4. Audit

| Инструмент | Команда | Частота |
|---|---|---|
| Rust | `cargo audit` | CI + раз в месяц |
| Frontend | `pnpm audit` | CI + раз в месяц |

---

## 5. Добавление новой зависимости

### 5.1. Чек-лист

Перед добавлением новой библиотеки:

1. **Необходимость.** Задача не может быть решена утверждёнными зависимостями.
2. **Альтернативы.** Рассмотрены 2–3 альтернативы, выбрана лучшая.
3. **Качество.** Crate/пакет активно поддерживается (последний коммит < 6 месяцев).
4. **Популярность.** Достаточное количество пользователей (GitHub stars / downloads).
5. **Безопасность.** Нет известных уязвимостей (`cargo audit` / `pnpm audit`).
6. **Лицензия.** MIT, Apache-2.0 или BSD (совместимые). Нет GPL/AGPL в dependencies.
7. **Размер.** Не добавляет чрезмерный размер бинарника / bundle.
8. **Совместимость.** Не конфликтует с существующими зависимостями.

### 5.2. Процесс

1. Создать issue с обоснованием: зачем, какие альтернативы, почему выбрана.
2. Получить approval от техлида.
3. Добавить зависимость в `Cargo.toml` / `package.json`.
4. Запустить `cargo audit` / `pnpm audit`.
5. Реализовать функциональность.
6. Пройти code review (включая review новой зависимости).
7. Обновить этот документ (раздел 2 или 3).

### 5.3. Запрещённые зависимости

- `openssl` (использовать `rustls` вместо него).
- `chrono` v0.3 (устаревшая, использовать 0.4).
- Любой crate с GPL/AGPL лицензией.
- `reqwest` с `blocking` feature (использовать async).
- Крейты с `unsafe` кодом без обоснования.

---

## 6. Workspace структура

### 6.1. Rust

```toml
# Cargo.toml (workspace root)
[workspace]
members = ["backend"]

[workspace.dependencies]
axum = "0.8"
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "uuid", "chrono"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
```

### 6.2. Frontend

```json
// package.json
{
  "dependencies": {
    "react": "^19.0.0",
    "react-dom": "^19.0.0",
    "react-router": "^7.0.0",
    "@tanstack/react-query": "^5.0.0",
    "zustand": "^5.0.0",
    "i18next": "^24.0.0",
    "react-i18next": "^15.0.0",
    "lucide-react": "^0.460.0",
    "sonner": "^1.7.0"
  },
  "devDependencies": {
    "typescript": "^5.6.0",
    "vite": "^6.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "vitest": "^3.0.0",
    "@testing-library/react": "^16.0.0",
    "@testing-library/jest-dom": "^6.6.0",
    "tailwindcss": "^4.0.0"
  }
}
```

> `shadcn/ui` не в `package.json` — компоненты копируются в `src/components/ui/` через CLI `npx shadcn@latest add <component>`.

---

## 7. Анализ дерева зависимостей

### 7.1. Rust

```bash
# Дерево зависимостей
cargo tree

# Проверка дубликатов
cargo tree -d

# Audit уязвимостей
cargo audit
```

### 7.2. Frontend

```bash
# Дерево зависимостей
pnpm why <package>

# Audit
pnpm audit

# Анализ размера bundle
pnpm build
npx vite-bundle-visualizer
```

---

## 8. Запрет на неутверждённые зависимости

- Неутверждённые зависимости в PR → request changes.
- Добавление зависимости без issue/approval → request changes.
- Обновление MAJOR версии без отдельного PR → request changes.
- Удаление утверждённой зависимости без обоснования → request changes.

---

## References

- `docs/ARCHITECTURE.md` — технологический стек
- `docs/CODE_STYLE.md` — конвенции кода
- `docs/CODE_REVIEW.md` — чек-лист review
- `Cargo.toml` — backend dependencies
- `frontend/package.json` — frontend dependencies