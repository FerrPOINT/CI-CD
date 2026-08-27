# Реестр визуальных evidence

> Скриншоты сняты Playwright (Chromium) с живого стенда и deterministic seed (`frontend/scripts/seed-evidence.mjs`). Desktop 1920×1080 full-page, mobile 375×812. Локаль ru, тема dark, DPR 1. Пересъёмка: `node frontend/scripts/shoot-evidence.mjs`.

## Страницы (базовые состояния)

| Файл | Маршрут | Что показывает | Размер |
|---|---|---|---|
| [01-login.png](../../screenshots/01-login.png) | `/login` | Вход | 1920×1080 |
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
| [16-schedules.png](../../screenshots/16-schedules.png) | `/projects/:id/schedules` | Расписания | 1920×1080 |
| [17-webhooks.png](../../screenshots/17-webhooks.png) | `/projects/:id/webhooks` | Webhooks + уведомления | 1920×1080 |
| [18-reports.png](../../screenshots/18-reports.png) | `/projects/:id/reports` | Отчёты | 1920×1080 |
| [19-audit-log.png](../../screenshots/19-audit-log.png) | `/audit-log` | Журнал аудита | 1920×1481 |
| [20-users.png](../../screenshots/20-users.png) | `/users` | Пользователи и API-токены | 1920×1080 |
| [21-artifacts.png](../../screenshots/21-artifacts.png) | `/jobs/:jobId/artifacts` | Артефакты | 1920×1080 |

## Состояния действий (диалоги, формы, панели)

| Файл | Маршрут + действие | Что показывает | Размер |
|---|---|---|---|
| [22-pr-diff.png](../../screenshots/22-pr-diff.png) | PR → «Посмотреть изменения» | Diff из PR: ветки, merge-base, патч | 1920×1080 |
| [23-project-create.png](../../screenshots/23-project-create.png) | Проекты → «Создать проект» | Форма создания проекта | 1920×1080 |
| [24-project-delete-confirm.png](../../screenshots/24-project-delete-confirm.png) | Проекты → «Удалить» | Диалог подтверждения удаления | 1920×1080 |
| [25-repo-create.png](../../screenshots/25-repo-create.png) | Репозитории → «Создать репозиторий» | Форма создания репозитория | 1920×1080 |
| [26-runner-register.png](../../screenshots/26-runner-register.png) | Runner-ы → «Зарегистрировать runner» | Форма регистрации runner | 1920×1080 |
| [27-secret-add.png](../../screenshots/27-secret-add.png) | Секреты → «Добавить секрет» | Форма добавления секрета | 1920×1080 |
| [28-env-create.png](../../screenshots/28-env-create.png) | Окружения → «Создать окружение» | Форма создания окружения | 1920×1080 |
| [29-schedule-create.png](../../screenshots/29-schedule-create.png) | Расписания → «Создать расписание» | Форма создания расписания | 1920×1080 |
| [30-webhook-add.png](../../screenshots/30-webhook-add.png) | Webhooks → «Добавить webhook» | Форма добавления webhook | 1920×1080 |
| [31-pr-create.png](../../screenshots/31-pr-create.png) | PR → «Создать pull-запрос» | Форма создания PR (ветки/заголовок/описание) | 1920×2264 |
| [32-user-create.png](../../screenshots/32-user-create.png) | Пользователи → «Создать пользователя» | Форма создания пользователя | 1920×1080 |
| [33-job-logs.png](../../screenshots/33-job-logs.png) | Пайплайн → «Логи» | Панель логов джоба с реальным выводом | 1920×1080 |
| [34-pipeline-run-form.png](../../screenshots/34-pipeline-run-form.png) | Пайплайны → «Запустить пайплайн» | Форма запуска (git ref) | 1920×1080 |

## Мобильные версии

| Файл | Маршрут | Что показывает | Размер |
|---|---|---|---|
| [m-dashboard.png](../../screenshots/m-dashboard.png) | `/` | Дашборд — мобильная версия | 375×1438 |
| [m-projects.png](../../screenshots/m-projects.png) | `/projects` | Проекты — мобильная версия | 375×1930 |
| [m-pipeline-detail.png](../../screenshots/m-pipeline-detail.png) | `/pipelines/:id` | Детали пайплайна — мобильная версия | 375×855 |
| [m-runners.png](../../screenshots/m-runners.png) | `/runners` | Runners — мобильная версия (карточный layout) | 375×812 |
| [m-pull-request.png](../../screenshots/m-pull-request.png) | `/repositories/:repo/pulls/:number` | Pull-запрос — мобильная версия | 375×812 |
