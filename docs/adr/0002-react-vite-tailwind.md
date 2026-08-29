# ADR-0002: React + Vite + Tailwind + shadcn/ui для Dashboard

## Status

Accepted; current screen set expanded beyond the original manual-transition MVP.

## Context

Dashboard отображает проекты, пайплайны, стадии, задачи и логи. В момент принятия ADR он также управлял ручными переходами статусов MVP; текущий Dashboard уже покрывает embedded runner, Git, secrets, artifacts, schedules/webhooks MVP, users/tokens и settings. Нужен быстрый frontend без тяжёлого server runtime, с типизированными компонентами, доступными базовыми UI-примитивами и предсказуемой сборкой для self-hosted Docker deployment.

## Alternatives Considered

| Option | Pros | Cons |
|---|---|---|
| Vue 3 + Vite | Низкий порог входа, хорошая реактивность, зрелая экосистема | Не использует выбранную React-экосистему и доступные React-компоненты команды |
| Svelte / SvelteKit | Компактный код, быстрая отрисовка | Меньший рынок компонентов и опыта; SvelteKit добавляет server/runtime решения, не нужные Dashboard MVP |
| React 19 + Next.js | Полный framework, SSR возможности | Избыточен для SPA control plane, усложняет self-hosted runtime |
| React 19 + Vite 6 + Tailwind 4 + shadcn/ui | Зрелая React-экосистема, быстрый dev/build loop, композиция доступных UI-примитивов | Нужно самостоятельно поддерживать архитектуру SPA и консистентность компонентов |

## Decision

Использовать React 19 с TypeScript, Vite 6, Tailwind CSS 4 и shadcn/ui. Dashboard собирается в статические файлы и в production обслуживается nginx. Vite proxy в development направляет `/api` в Rust API на `http://localhost:22801`.

shadcn/ui применяется как набор исходных компонентов и паттернов, а не непрозрачная runtime-библиотека: код компонентов остаётся в репозитории и может адаптироваться к задаче Dashboard. Tailwind даёт общий набор design tokens и утилит без отдельного тяжёлого CSS-runtime.

## Consequences

- Команда использует типизированный React UI и широкий набор совместимых библиотек.
- Dev server и production build быстры; итоговый контейнер frontend не требует Node.js runtime.
- Нужно поддерживать единые паттерны для loading/error/empty состояний, accessibility и responsive layout.
- Зависимости pnpm lockfile обязательны для воспроизводимого CI; проверки включают `pnpm test` и `pnpm build`.
- SSR и server actions отсутствуют намеренно; если они станут требованиями, решение пересматривается отдельным ADR.

## Related

- `docs/UI_UX.md`
- `docs/CODE_STYLE.md`
- `docs/TESTING.md`
- `docs/CI_CD.md`
