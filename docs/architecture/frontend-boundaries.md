# Frontend boundaries

> **Статус:** объяснительный документ. Канон — `docs/contracts/UI_API_CONTRACT.md`.

## Текущая структура (verified)

```text
frontend/src/
├── api/          # типизированный клиент + TanStack Query hooks (hand-written)
├── app/          # router.tsx (21 маршрут), providers
├── pages/        # 20 страниц + login (feature-папки)
├── shared/       # ui-kit (shadcn), i18n (ru/en), theme
└── widgets/      # AppShell (sidebar/header/Outlet)
```

## Правила границ

- `pages/*` не делают fetch напрямую — только через `api/` hooks.
- `shared/ui` не импортирует `api`/`pages`.
- i18n: все пользовательские строки через `t()`; ключи en/ru в паритете (проверяется verify_docs + vitest).
- Статусы отображаются текстом + цветом (не только цветом).

## Целевое (approved)

- Типы транспорта генерируются из `openapi/openapi.yaml` в `shared/api/generated/`; hand-written типы удаляются по мере миграции endpoint-групп.
- Query-key конвенции и инвалидации — `contracts/UI_API_CONTRACT.md`.
- Live-обновления (pipeline/job logs) — SSE projection (target, `AUTOMATION_ARCHITECTURE.md`).
- Responsive/a11y-требования — `docs/DELIVERY_ARCHITECTURE.md` + USER_GUIDE.md (Gate 4/5 evidence).
