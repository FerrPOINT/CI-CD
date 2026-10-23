# Frontend boundaries

> **Статус:** объяснительный документ. Канон — `docs/contracts/UI_API_CONTRACT.md`.

## Текущая структура (verified)

```text
frontend/src/
├── api/          # typed wrapper/hooks + generated OpenAPI DTO schema.d.ts
├── app/          # router.tsx (21 маршрут), route smoke, providers
├── pages/        # 20 рабочих страниц + login (feature-папки)
├── shared/       # продуктовые UI-адаптеры, i18n (ru/en), подключение Base
└── widgets/      # AppShell (sidebar/header/Outlet)
```

## Правила границ

- Native поле с иконкой действия использует InputWithAction Base. В редакторе
  секретов Forge сохраняет value/type, showValue, callbacks и pending; Base задаёт
  геометрию, доступный trigger и native ref. Раскрытие не запускает запись и не
  создаёт input/change. Для поиска использовать SearchInput с готовой очисткой.

- `pages/*` не делают fetch напрямую — только через `api/` hooks.
- `shared/ui` не импортирует `api`/`pages`.
- Общие primitives, states, tokens и каркас импортируются из публичного `@pdlc/ui`.
  `shared/ui` сохраняет предметные адаптеры: например, QueryState с пояснением
  запрета доступа Forge и CapabilityCallout с продуктовым статусом capability.
  Оформление Notice, LoadingState, ErrorState и EmptyState принадлежит Base;
  запросы, права, retry и неизвестный результат операции — Forge.
- i18n: все пользовательские строки через `t()`; ключи en/ru в паритете, leaf-значения не пустые, runtime switch i18next и динамические ключи для стабильных API contract values покрыты `frontend/src/shared/i18n/i18n-contract.test.ts`.
- Статусы отображаются текстом + цветом (не только цветом).
- Страница должна показывать фактический статус capability: `Current verified`, `Configuration only` или `Target approved`; target-функции не оформляются как готовые action controls.
- Для страниц с частично реализованной capability используется общий `CapabilityCallout` из `frontend/src/shared/ui/capability-callout.tsx`: `Runners` показывает registry/heartbeat как MVP, `Environments` — metadata/history/protected approval/rollback MVP без расширенной policy orchestration, `Schedules` — scheduler MVP, `Webhooks` — outgoing delivery MVP, `Delivery history` — outbox history/requeue MVP, `Notifications` — `in_app`/`sse` delivery MVP.
- Основной desktop layout — рабочий dashboard/sidebar с плотными таблицами и формами; mobile — карточный список без потери create/view/retry/cancel сценариев.
- Новые страницы добавляются только для самостоятельного CI/CD workflow. Справка, issue tracking, registry и IDE-экраны остаются out-of-scope.
- Каждый route из текущего baseline должен проходить `frontend/src/app/router.test.tsx`: тест использует production `appRoutes`, memory router и mocked API DTO для первого рендера 20 рабочих страниц + `/login`. Critical real-browser journeys проверяются `frontend/e2e/critical-flows.spec.ts` на собранном Compose stack; all-route desktop a11y smoke — `frontend/e2e/accessibility.spec.ts`.

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

## UI Shell Contract

Forge CI/CD follows the Base [UI Shell Standard](https://github.com/FerrPOINT/services-base/blob/main/docs/platform/UI_SHELL_STANDARD.md).
`AppShell` owns the shared 264 px/72 px permission-aware navigation, 60 px
header and full-width right work area. Routes use only `wide`, `reading/form`
or `detail-with-aside`; they do not select Forge-specific shell geometry.

- Repositories, pipelines, runners, jobs, audit and logs use `wide`. Tables/logs
  keep deliberate horizontal scroll local and use `minmax(0, 1fr)` so the
  document itself never overflows.
- Detail routes use `detail-with-aside`: a fluid primary pane plus a 320 px side
  context panel. Settings/forms use `reading/form`, with only a 760 px inner
  column bounded without narrowing data pages.
- Expanded desktop sidebar, compact tablet rail and mobile drawer keep the same
  navigation order and active state. Drawer behavior includes keyboard focus,
  Escape close and focus restoration.
- Service/profile/theme controls stay in the single global header row. Page
  title, breadcrumbs, filters and run actions remain in page-owned rows below.
- Shell/layout evidence covers 375, 1440 and 2560 px, direct-route active nav,
  one-row header alignment and no document-level horizontal overflow.

## References

- [UI/API Contract](../contracts/UI_API_CONTRACT.md) — query and state boundary.
- [User Guide](../USER_GUIDE.md) — operator route behavior.
- [Base UI Shell Standard](https://github.com/FerrPOINT/services-base/blob/main/docs/platform/UI_SHELL_STANDARD.md) — shared shell rules.


## Общие размеры и переход по знаку платформы

Button/Input и TabsTrigger используют размеры и typography/padding Base.
Служебные h-10/min-h-10 и прежние smaller responsive overrides не копируются
в owner pages/dev-preview. Ширина дерева/таблиц/панелей и композиция содержимого
остаются Forge. Custom PlatformHeader оформляет native NavLink логотипа через
Button asChild/ghost/icon; routing и namespace не заменяются кнопочными handlers.
Оставшиеся icon actions и file/tree compositions классифицируются отдельно:
наличие общего Button не разрешает копировать вокруг него skin.

Иконки submit/очистки/перелистывания repository browser, скачивания артефактов,
раскрытия deployments и редактирования releases/schedules/secrets используют
Button size=icon. Локальные width/padding/hover/focus-скины удалены; callbacks,
aria-label, disabled/pending, URL и submit остаются прежними. Это source-перенос,
а не установка UI pin или разрешение сетевых операций в QA.
