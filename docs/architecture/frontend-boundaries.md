# Frontend boundaries

> **Статус:** объяснительный документ. Канон — `docs/contracts/UI_API_CONTRACT.md`.

## Текущая структура (verified)

```text
frontend/src/
├── api/          # typed wrapper/hooks + generated OpenAPI DTO schema.d.ts
├── app/          # router.tsx (21 маршрут), route smoke, providers
├── pages/        # 20 рабочих страниц + login (feature-папки)
├── shared/       # ui-kit (shadcn), i18n (ru/en), theme
└── widgets/      # AppShell (sidebar/header/Outlet)
```

## Правила границ

- `pages/*` не делают fetch напрямую — только через `api/` hooks.
- `shared/ui` не импортирует `api`/`pages`.
- i18n: все пользовательские строки через `t()`; ключи en/ru в паритете (проверяется verify_docs + vitest).
- Статусы отображаются текстом + цветом (не только цветом).
- Страница должна показывать фактический статус capability: `Current verified`, `Configuration only` или `Target approved`; target-функции не оформляются как готовые action controls.
- Для страниц с частично реализованной capability используется общий `CapabilityCallout` из `frontend/src/shared/ui/capability-callout.tsx`: `Runners` показывает registry/heartbeat как MVP, `Schedules` — scheduler MVP, `Webhooks` — outgoing delivery MVP, `Delivery history` — outbox history/requeue MVP, `Notifications` — `in_app`/`sse` delivery MVP.
- Основной desktop layout — рабочий dashboard/sidebar с плотными таблицами и формами; mobile — карточный список без потери create/view/retry/cancel сценариев.
- Новые страницы добавляются только для самостоятельного CI/CD workflow. Справка, issue tracking, registry и IDE-экраны остаются out-of-scope.
- Каждый route из текущего baseline должен проходить `frontend/src/app/router.test.tsx`: тест использует production `appRoutes`, memory router и mocked API DTO для первого рендера 20 рабочих страниц + `/login`. Critical real-browser journeys проверяются `frontend/e2e/critical-flows.spec.ts` на собранном Compose stack; representative a11y smoke — `frontend/e2e/accessibility.spec.ts`.

## Page-design baseline

| Группа | Страницы | Назначение |
|---|---|---|
| Core CI | Dashboard, Projects, Pipelines, Pipeline detail, Job logs, Artifacts | Запуск, наблюдение, диагностика и evidence выполнения. |
| Source | Repositories, Repository browser, Compare, Pull requests, Pull request detail | Минимальная поддержка Git-потока, не полноценная замена code-review платформы. |
| Execution & security | Runners, Secrets, Project members, Users, Audit log, Login | Исполнение, доступ, секреты и расследование действий. |
| Delivery | Environments, Schedules, Webhooks, Reports | Метаданные доставки, automation config/outcomes и базовая отчётность. |
| Support | Settings | Системный срез портов и `CICD_` переменных без отдельного административного дубля. |

Отдельный статичный `/admin` не входит в baseline: административная страница добавляется только вместе с реальным workflow управления instance/tenant/policy, а не как справочная витрина.

## Целевое (approved)

- DTO генерируются из `openapi/openapi.yaml` в `frontend/src/api/schema.d.ts`; generated transport boundary остаётся target.
- Query-key конвенции и инвалидации — `contracts/UI_API_CONTRACT.md`.
- Live-обновления job logs и `in_app`/`sse` notifications — current SSE endpoints; общая pipeline/events projection остаётся target (`AUTOMATION_ARCHITECTURE.md`).
- Responsive/a11y-требования — `docs/DELIVERY_ARCHITECTURE.md` + USER_GUIDE.md (Gate 4/5 evidence).
