# Реестр визуальных evidence

> Скриншоты сняты Playwright (Chromium) с живого disposable стенда и deterministic seed (`frontend/scripts/seed-evidence.mjs`). Viewport: desktop 1920×1080 и mobile 375×812; итоговая высота full-page PNG может быть больше viewport. Локаль ru, тема dark, DPR 1. Пересъёмка: `cd frontend && pnpm seed:evidence && pnpm shoot:evidence`. Для быстрой пересъёмки только README-набора используется `EVIDENCE_README_ONLY=true`.
>
> Номер `08` зарезервирован за удалённым статичным `/admin` screen. Отдельный `/admin` не входит в текущий baseline; системный срез находится в `/settings`.

## Страницы (базовые состояния)

| Файл | Маршрут | Что показывает | Размер |
|---|---|---|---|
| [02-dashboard.png](../../screenshots/02-dashboard.png) | `/` | Дашборд с метриками запусков | 1920×1080 |
| [03-projects.png](../../screenshots/03-projects.png) | `/projects` | Проекты | 1920×1080 |
| [04-repositories.png](../../screenshots/04-repositories.png) | `/repositories` | Репозитории | 1920×1080 |
| [05-pipelines.png](../../screenshots/05-pipelines.png) | `/projects/:id/pipelines` | Пайплайны проекта | 1920×1080 |
| [06-pipeline-detail.png](../../screenshots/06-pipeline-detail.png) | `/pipelines/:id` | Детали пайплайна: immutable plan snapshot, стадии, jobs, команды, runner tags, declared secrets и artifacts | 1920×1080 |
| [07-settings.png](../../screenshots/07-settings.png) | `/settings` | Системные настройки и `CICD_` окружение | 1920×1110 |
| [09-repository-browser.png](../../screenshots/09-repository-browser.png) | `/repositories/:repo` → «Код» | Дерево и preview Git-файлов | 1920×1080 |
| [10-compare.png](../../screenshots/10-compare.png) | `/repositories/:repo/compare` | Сравнение веток: diff + статистика | 1920×1080 |
| [11-pull-requests.png](../../screenshots/11-pull-requests.png) | `/repositories/:repo/pulls` | Pull-запросы | 1920×1080 |
| [12-pull-request-detail.png](../../screenshots/12-pull-request-detail.png) | `/repositories/:repo/pulls/:number` | Pull-запрос: карточка и действия | 1920×1080 |
| [13-runners.png](../../screenshots/13-runners.png) | `/runners` | Runners | 1920×1080 |
| [14-secrets.png](../../screenshots/14-secrets.png) | `/projects/:id/secrets` | Секреты проекта | 1920×1080 |
| [15-environments.png](../../screenshots/15-environments.png) | `/projects/:id/environments` | Окружения и деплои | 1920×1080 |
| [16-schedules.png](../../screenshots/16-schedules.png) | `/projects/:id/schedules` | Расписания | 1920×1080 |
| [17-webhooks.png](../../screenshots/17-webhooks.png) | `/projects/:id/webhooks` | Webhooks + история доставок + уведомления | 1920×1255 |
| [18-reports.png](../../screenshots/18-reports.png) | `/projects/:id/reports` | Отчёты | 1920×1080 |
| [19-audit-log.png](../../screenshots/19-audit-log.png) | `/audit-log` | Журнал аудита | 1920×1080 |
| [20-users.png](../../screenshots/20-users.png) | `/users` | Переход к управлению пользователями в Admin Panel | 1920×1080 |
| [21-artifacts.png](../../screenshots/21-artifacts.png) | `/jobs/:jobId/artifacts` | Артефакты | 1920×1080 |

## Состояния действий (диалоги, формы, панели)

| Файл | Маршрут + действие | Что показывает | Размер |
|---|---|---|---|
| [22-pr-diff.png](../../screenshots/22-pr-diff.png) | PR → «Посмотреть изменения» | Inline diff конкретного PR: номер, ветки, merge-base, patch | 1920×1080 |
| [23-project-create.png](../../screenshots/23-project-create.png) | Проекты → «Создать проект» | Форма создания проекта | 1920×1080 |
| [24-project-delete-confirm.png](../../screenshots/24-project-delete-confirm.png) | Проекты → «Удалить» | Диалог подтверждения удаления | 1920×1080 |
| [25-repo-create.png](../../screenshots/25-repo-create.png) | Репозитории → «Создать репозиторий» | Форма создания репозитория | 1920×1080 |
| [26-runner-register.png](../../screenshots/26-runner-register.png) | Runner-ы → «Зарегистрировать runner» | Форма регистрации runner | 1920×1080 |
| [27-secret-add.png](../../screenshots/27-secret-add.png) | Секреты → «Добавить секрет» | Форма добавления секрета | 1920×1080 |
| [28-env-create.png](../../screenshots/28-env-create.png) | Окружения → «Создать окружение» | Форма создания окружения | 1920×1080 |
| [29-schedule-create.png](../../screenshots/29-schedule-create.png) | Расписания → «Создать расписание» | Форма создания расписания | 1920×1080 |
| [30-webhook-add.png](../../screenshots/30-webhook-add.png) | Webhooks → «Добавить webhook» | Форма добавления webhook | 1920×1080 |
| [31-pr-create.png](../../screenshots/31-pr-create.png) | PR → «Создать pull-запрос» | Форма создания PR (ветки/заголовок/описание) | 1920×1612 |
| [33-job-logs.png](../../screenshots/33-job-logs.png) | Пайплайн → «Логи» | Панель логов джоба с runner tags, secret names, artifact paths, выводом и поиском | 1920×1080 |
| [34-pipeline-run-form.png](../../screenshots/34-pipeline-run-form.png) | Пайплайны → «Запустить пайплайн» | Форма запуска (git ref) | 1920×1080 |
| [35-releases-list.png](../../screenshots/35-releases-list.png) | Репозиторий → «Релизы» | Список release metadata и Git tags | 1920×1080 |
| [36-repo-code-blob.png](../../screenshots/36-repo-code-blob.png) | Репозиторий → Код → `.forge-ci.yml` | Просмотр YAML файла CI | 1920×1080 |
| [37-repo-tags.png](../../screenshots/37-repo-tags.png) | Репозиторий → «Теги» | Список Git tags | 1920×1080 |
| [38-releases-create.png](../../screenshots/38-releases-create.png) | Репозиторий → Релизы → «Создать релиз» | Диалог release metadata | 1920×1080 |
| [39-repo-code-src.png](../../screenshots/39-repo-code-src.png) | Репозиторий → Код → `src` | Навигация по подкаталогу | 1920×1080 |
| [41-env-approval-controls.png](../../screenshots/41-env-approval-controls.png) | Окружения → «Деплои» | Protected deployment approval controls и rollback action | 1920×1080 |
| [23-forbidden.png](../../screenshots/23-forbidden.png) | `/forbidden` | 403 — доступ запрещён (роль без прав) | 1920×1080 |

## Responsive modes

Namespace candidate добавляет `/namespace`, `/catalog/repositories/:id` и
`/catalog/repositories/:id/pulls/:number`. Визуальная приёмка этих маршрутов
в целевом `pdlc1` через Codex IAB на 375/1920/2560 ещё не выполнена;
исторические screenshots выше не подтверждают эти новые экраны.

Workspace/configuration candidate добавляет `/workspaces/:registry/:namespace/*`,
`/delivery-configs`, `/delivery-configs/:projectId/settings`,
`/delivery-configs/:projectId/pipelines`, `/delivery-configs/:projectId/secrets`,
`/delivery-configs/:projectId/environments`, `/delivery-configs/:projectId/schedules`,
`/delivery-configs/:projectId/webhooks` и `/delivery-configs/:projectId/reports`.
Их актуальные снимки и проверка клавиатуры/focus на живом candidate ещё не сняты.
До приёмки на 375/1920/2560 эти маршруты остаются неподтверждёнными;
исторические изображения legacy `/projects/:id/...` не заменяют эту проверку.

| Файл | Маршрут | Режим | Viewport | Размер PNG |
|---|---|---|---|---|
| [wide.png](../../screenshots/375x812/wide.png) | `/` | `wide` | 375×812 | 375×1306 |
| [reading.png](../../screenshots/375x812/reading.png) | `/settings` | `reading` | 375×812 | 375×1886 |
| [detail-with-aside.png](../../screenshots/375x812/detail-with-aside.png) | `/pipelines/:id` | `detail-with-aside` | 375×812 | 375×1032 |

## Синхронизация репозиториев — установленный SDLC2, 17 октября 2026

Снимки Codex IAB показывают реальный установленный UI `178efff`: пустой список
подключений и форму существующего QA-репозитория. Настройки и credentials
не сохранялись, sync не запускался. Это evidence композиции на 375/1920/2560,
а не подтверждение backend-исправлений объединённого кандидата или обмена с
GitHub/GitLab. Весь токеновый ввод оставлен пустым. PNG получены непосредственно
через browser capture; размеры — full-page, DPR 1.

| Файл | Маршрут | Режим | Viewport | Размер PNG |
|---|---|---|---|---|
| [42-repository-syncs-375.png](../../screenshots/42-repository-syncs-375.png) | `/repository-syncs` | Пустой список | 375×812 | 375×812 |
| [42-repository-syncs-1920.png](../../screenshots/42-repository-syncs-1920.png) | `/repository-syncs` | Пустой список | 1920×1080 | 1920×1080 |
| [42-repository-syncs-2560.png](../../screenshots/42-repository-syncs-2560.png) | `/repository-syncs` | Пустой список | 2560×1440 | 2560×1440 |
| [43-repository-sync-connection-375.png](../../screenshots/43-repository-sync-connection-375.png) | `/catalog/repositories/:id/sync` | Подключение отключено | 375×812 | 375×1395 |
| [43-repository-sync-connection-1920.png](../../screenshots/43-repository-sync-connection-1920.png) | `/catalog/repositories/:id/sync` | Подключение отключено | 1920×1080 | 1920×1080 |
| [43-repository-sync-connection-2560.png](../../screenshots/43-repository-sync-connection-2560.png) | `/catalog/repositories/:id/sync` | Подключение отключено | 2560×1440 | 2560×1440 |

## Синхронизация — текущий source-компонент, 17 октября 2026

Проверен настоящий `RepositorySyncEditor` из frontend tree
`b6f62ff3ff23d6bbd11723299f9bc8732cb6b513` с Base UI `2ad0834`.
В отдельном Chromium profile прошли сохранение, проверка доступа и переход
к истории. Транспорт фикстурный; banner на снимках явно обозначает вымышленные
данные. Проверка не подтверждает реальный provider или SSO нового backend.
Статические bytes передавались из собственного QA-сервера с сохранением
loopback browser origin; crypto API не заменялся. Внешних запросов и ошибок
страницы нет, горизонтальной прокрутки нет. QA-проект очищен.

| Файл | Маршрут | Режим | Viewport | Размер PNG |
|---|---|---|---|---|
| [44-repository-sync-source-375.png](../../screenshots/44-repository-sync-source-375.png) | `/sync-preview.html?production=1` | Production-компонент, fixture transport | 375×812 | 375×1486 |
| [44-repository-sync-source-1920.png](../../screenshots/44-repository-sync-source-1920.png) | `/sync-preview.html?production=1` | Production-компонент, fixture transport | 1920×1080 | 1920×1080 |
| [44-repository-sync-source-2560.png](../../screenshots/44-repository-sync-source-2560.png) | `/sync-preview.html?production=1` | Production-компонент, fixture transport | 2560×1440 | 2560×1440 |

## Пользователи и каталог сервисов — установленный SDLC2, 17 октября 2026

Снимки настоящего UI из CI/CD `1216eee` и Base UI `e0759e0` получены
в отдельном Chromium profile на собственном зарегистрированном origin
`http://localhost:8712`. Auth и каталог Admin настоящие; данные на снимках
не заменялись фикстурами. Открыто штатное меню сервисов, горизонтальной
прокрутки нет. PNG — непосредственный full-page capture, DPR 1.

В Chromium, Firefox и WebKit отдельно прошли SSO callback, точный Admin URL
с namespace, reload, запрос к собственному API, logout и отклонение отозванного
bearer. Две проверки Chromium дополнительно задерживают или отклоняют только
публичный GET каталога: до подтверждения назначения недоступны; после
возобновления настоящего GET собственный Admin URL восстанавливается.
Настройки безопасности браузеров сохранены, внешних AI-вызовов нет.
Эти проверки не подтверждают полный автоматический SDLC.

| Файл | Маршрут | Режим | Viewport | Размер PNG |
|---|---|---|---|---|
| [45-users-catalog-375.png](../../screenshots/45-users-catalog-375.png) | `/users` | Настоящий runtime catalog, меню сервисов | 375×900 | 375×900 |
| [45-users-catalog-1920.png](../../screenshots/45-users-catalog-1920.png) | `/users` | Настоящий runtime catalog, меню сервисов | 1920×900 | 1920×900 |
| [45-users-catalog-2560.png](../../screenshots/45-users-catalog-2560.png) | `/users` | Настоящий runtime catalog, меню сервисов | 2560×900 | 2560×900 |
