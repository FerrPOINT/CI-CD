# Реестр визуальных evidence

> Скриншоты сняты Playwright (Chromium). Коммит: `f850fbf 2026-08-27`. Desktop 1920×1080 full-page, mobile 375×812. Локаль ru, тема dark, DPR 1. Пересъёмка: `frontend/e2e/screenshots.spec.ts` (см. DEVELOPMENT_GUIDE).

| Файл | Маршрут | Что показывает | Viewport |
|---|---|---|---|
| [01-login.png](../../screenshots/01-login.png) | `/login` | Вход (UI-заглушка) | 1440×900 |
| [02-dashboard.png](../../screenshots/02-dashboard.png) | `/` | Дашборд | 1440×900 |
| [03-projects.png](../../screenshots/03-projects.png) | `/projects` | Проекты | 1440×900 |
| [04-repositories.png](../../screenshots/04-repositories.png) | `/repositories` | Репозитории | 1440×900 |
| [05-pipelines.png](../../screenshots/05-pipelines.png) | `/projects/:id/pipelines` | Пайплайны | 1440×900 |
| [06-pipeline-detail.png](../../screenshots/06-pipeline-detail.png) | `/pipelines/:id` | Детали пайплайна | 1440×900 |
| [07-settings.png](../../screenshots/07-settings.png) | `/settings` | Настройки | 1440×900 |
| [08-admin.png](../../screenshots/08-admin.png) | `/admin` | Администрирование (справка) | 1440×900 |
| [09-repository-browser.png](../../screenshots/09-repository-browser.png) | `/repositories/:repo` | Коммиты и ветки | 1440×900 |
| [10-compare.png](../../screenshots/10-compare.png) | `/repositories/:repo/compare` | Сравнение веток | 1440×900 |
| [11-pull-requests.png](../../screenshots/11-pull-requests.png) | `/repositories/:repo/pulls` | Pull-запросы | 1440×900 |
| [12-repositories-filtered.png](../../screenshots/12-repositories-filtered.png) | `/repositories?project=` | Репозитории (фильтр) | 1440×900 |
| [13-runners.png](../../screenshots/13-runners.png) | `/runners` | Runners | 1920×1080 |
| [14-secrets.png](../../screenshots/14-secrets.png) | `/projects/:id/secrets` | Секреты проекта | 1920×1080 |
| [15-artifacts.png](../../screenshots/15-artifacts.png) | `/jobs/:jobId/artifacts` | Артефакты | 1920×1080 |
| [16-environments.png](../../screenshots/16-environments.png) | `/projects/:id/environments` | Окружения | 1920×1080 |
| [17-schedules.png](../../screenshots/17-schedules.png) | `/projects/:id/schedules` | Расписания (config only) | 1920×1080 |
| [18-webhooks.png](../../screenshots/18-webhooks.png) | `/projects/:id/webhooks` | Webhooks (config only) | 1920×1080 |
| [19-reports.png](../../screenshots/19-reports.png) | `/projects/:id/reports` | Отчёты | 1920×1080 |
| [20-audit-log.png](../../screenshots/20-audit-log.png) | `/audit-log` | Журнал аудита | 1920×1080 |
| [21-users.png](../../screenshots/21-users.png) | `/users` | Пользователи и API-токены | 1920×1080 |
| [m-dashboard.png](../../screenshots/m-dashboard.png) | `/` | Дашборд — мобильная версия | 375×1084 |
| [m-projects.png](../../screenshots/m-projects.png) | `/projects` | Проекты — мобильная версия | 375×1156 |
| [m-pipeline-detail.png](../../screenshots/m-pipeline-detail.png) | `/pipelines/:id` | Детали пайплайна — мобильная версия | 375×812 |
| [m-runners.png](../../screenshots/m-runners.png) | `/runners` | Runners — мобильная версия (карточный layout) | 375×812 |
