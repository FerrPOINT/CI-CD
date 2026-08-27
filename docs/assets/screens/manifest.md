# Реестр визуальных evidence

> Скриншоты сняты Playwright (Chromium) с живого стенда и deterministic seed (`frontend/scripts/seed-evidence.mjs`). Коммит: `b4f85ca 2026-08-27`. Desktop 1920×1080 full-page, mobile 375×812. Локаль ru, тема dark, DPR 1. Пересъёмка: `node frontend/scripts/shoot-evidence.mjs`.

| Файл | Маршрут | Что показывает | Размер |
|---|---|---|---|
| [01-login.png](../../screenshots/01-login.png) | `/login` | Вход (UI-заглушка; auth — target) | 1920×1080 |
| [02-dashboard.png](../../screenshots/02-dashboard.png) | `/` | Дашборд с метриками запусков | 1920×1080 |
| [03-projects.png](../../screenshots/03-projects.png) | `/projects` | Проекты | 1920×1080 |
| [04-repositories.png](../../screenshots/04-repositories.png) | `/repositories` | Репозитории | 1920×1080 |
| [05-pipelines.png](../../screenshots/05-pipelines.png) | `/projects/:id/pipelines` | Пайплайны проекта | 1920×1080 |
| [06-pipeline-detail.png](../../screenshots/06-pipeline-detail.png) | `/pipelines/:id` | Детали пайплайна: стадии, jobs, команды | 1920×1080 |
| [07-settings.png](../../screenshots/07-settings.png) | `/settings` | Настройки (тема/язык) | 1920×1080 |
| [08-admin.png](../../screenshots/08-admin.png) | `/admin` | Администрирование (справка) | 1920×1080 |
| [09-repository-browser.png](../../screenshots/09-repository-browser.png) | `/repositories/:repo` | Коммиты и ветки | 1920×1080 |
| [10-compare.png](../../screenshots/10-compare.png) | `/repositories/:repo/compare` | Сравнение веток: diff + статистика | 1920×1080 |
| [11-pull-requests.png](../../screenshots/11-pull-requests.png) | `/repositories/:repo/pulls` | Pull-запросы | 1920×1408 |
| [12-pull-request-detail.png](../../screenshots/12-pull-request-detail.png) | `/repositories/:repo/pulls/:number` | Pull-запрос: карточка и действия | 1920×1080 |
| [13-runners.png](../../screenshots/13-runners.png) | `/runners` | Runners | 1920×1080 |
| [14-secrets.png](../../screenshots/14-secrets.png) | `/projects/:id/secrets` | Секреты проекта | 1920×1080 |
| [15-environments.png](../../screenshots/15-environments.png) | `/projects/:id/environments` | Окружения и деплои | 1920×1080 |
| [16-schedules.png](../../screenshots/16-schedules.png) | `/projects/:id/schedules` | Расписания (config only) | 1920×1080 |
| [17-webhooks.png](../../screenshots/17-webhooks.png) | `/projects/:id/webhooks` | Webhooks + уведомления (config only) | 1920×1080 |
| [18-reports.png](../../screenshots/18-reports.png) | `/projects/:id/reports` | Отчёты | 1920×1080 |
| [19-audit-log.png](../../screenshots/19-audit-log.png) | `/audit-log` | Журнал аудита | 1920×1481 |
| [20-users.png](../../screenshots/20-users.png) | `/users` | Пользователи и API-токены | 1920×1080 |
| [21-artifacts.png](../../screenshots/21-artifacts.png) | `/jobs/:jobId/artifacts` | Артефакты | 1920×1080 |
| [m-dashboard.png](../../screenshots/m-dashboard.png) | `/` | Дашборд — мобильная версия | 375×1438 |
| [m-projects.png](../../screenshots/m-projects.png) | `/projects` | Проекты — мобильная версия | 375×1930 |
| [m-pipeline-detail.png](../../screenshots/m-pipeline-detail.png) | `/pipelines/:id` | Детали пайплайна — мобильная версия | 375×855 |
| [m-runners.png](../../screenshots/m-runners.png) | `/runners` | Runners — мобильная версия (карточный layout) | 375×812 |
| [m-pull-request.png](../../screenshots/m-pull-request.png) | `/repositories/:repo/pulls/:number` | Pull-запрос — мобильная версия | 375×812 |
