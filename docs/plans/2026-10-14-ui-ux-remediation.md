# Доработки UI/UX 2026-10-14

## Подтверждённая пустота на компонентах Base — 26 октября 2026

5 регионов используют существующий EmptyState Base: задачи PR, PR репозитория, проекты, репозитории и пустой отчёт с прежней ссылкой действия.
Условия показа, различия поиска/страницы/пустого каталога, тексты, права,
запросы и действия сохранены. Product read admission не перенесён в библиотеку.
Композиция отчёта использует message/description/action, без локальной рамки и paint.

UI pin: `430ef57512bca547e44f1a6d4c5878d52ee978ce`. Общий PageHeader в этом кандидате переносит длинный
context; исправление находится в Base и действует в React и SSR. SDK/skills pins
сохранены; source adoption не означает установку.

Scoped unit: 13 cases, TypeScript и применимые lint/contract checks PASS.
Полные emitted modules и типы всех 12 затронутых модулей сравнены; шесть
негативных контролей отвергают изменение read scope, условий, фильтра и расчёта.
IAB: 384 контекста для 16 регионов в трёх темах, четырёх ширинах и двух шкалах;
пять дополнительных состояний и клавиатурный переход/Back. В preview используются
точные JSX-фрагменты; данные синтетические. Это не backend/runtime acceptance.
Снимки 375/1920/2560 пересняты полным viewport после обнаружения tiling у capture.
Доказательства находятся в private evidence/confirmed-empty-regions-20261025.
Упаковка, обновлённые environment и итоговый межпакетный gate учитываются отдельно
в общем плане Base; этот документ не объявляет их завершёнными.

UX-06/08. Контекст PageHeader объясняет показатели подключённых проектов, добавлены ссылки на репозитории и CI-конфигурации. SQL и непривязанные ресурсы сохранены. Вход и callback SSO используют Base.

Общие контракты и тексты: [стандарт Base](https://github.com/FerrPOINT/services-base/blob/6b9dd35a92bb095debf166724e4ecb1cb0e8ceb0/docs/platform/UI_UX_STANDARD.md). UI pin: `6b9dd35a92bb095debf166724e4ecb1cb0e8ceb0`; SDK и skills pins сохранены.

Frontend: 215 тестов после исправления привязки состояния; TypeScript/lint,
build, frozen lockfile, OpenAPI check и compatibility с main. Девять browser
API-fixture сценариев в Chromium/Firefox/WebKit подтверждают смену конфигурации,
очистку секретного ввода, закрытие старого подтверждения и редактора расписания.
Исторический IAB общего анонимного входа на 320/375/1920/2560 не заменяет живую SSO-приёмку.

Зависимость: [Base PR #206](https://github.com/FerrPOINT/services-base/pull/206). Кандидат поставляется draft: полная межпродуктовая IAB-приёмка, SSO и 30 направлений переключения ещё не подтверждены. Backend не менялся; его полный локальный Rust gate был остановлен после ограничения на дополнительные Docker-группы. GitHub CI учитывается отдельно; отсутствие запуска из-за billing не объявляется PASS.

Работа и native QA выполняются в SDLC2 без новых Docker-групп. Кандидат не
установлен; merge ожидает обязательную интеграционную приёмку.

## Привязка состояния CI-конфигурации — 2026-10-15

Страница настроек привязана к immutable ID конфигурации через существующий
ConfigurationContext. При переходе к другой, уже загруженной конфигурации
старые формы и подтверждения перемонтируются: нельзя перенести введённое
значение секрета или действие над старым расписанием в новую область.
Обычный refresh метаданных той же конфигурации сохраняет ввод.
Три регрессии воспроизведены до исправления; дополнительная проверка защищает
черновик при refresh. После интеграции main `b5e6096` снова прошли все 215
frontend-тестов, lint/TypeScript/OpenAPI/build и девять браузерных сценариев.
Скриншоты 375/1920/2560 проверены. Browser fixture использует синтетическую
сессию в памяти и существующий product router; это не реальный SSO/portal gate.
Backend и OpenAPI соответствуют main, включая принятые retry guards.

## Генерализация UI

InitialsAvatar, ConfirmDialog, PageNavigation, useDebouncedValue и общий QueryState используются из Base. Локальный QueryState содержит только правило показа ForbiddenPage.

## Общие блоки текста и описаний

DiffBlock заменяет два одинаковых PatchView. Forge передаёт patch, accessible name и сообщение об усечении; цвета/прокрутка/строки принадлежат Base.

## Общие состояния каталогов — 26 октября 2026

11 участков в восьми модулях используют EmptyState и ResourceStateView Base:
сравнение, namespace, pipeline, pull request, каталог запросов, browser репозитория,
синхронизации и webhooks. EmptyState содержит прежнее действие сброса поиска:
удаляются только repository_search/repository_offset, namespace и tab сохраняются.
Отсутствующий репозиторий в embedded browser получает h2, обычная страница — h1.
SQL/API, проверки 404, фильтры, callbacks и populated rendering не менялись.

119 distinct scoped cases в семи файлах, TypeScript, scoped ESLint и UI contract
PASS. Тест pull-request-detail проверяет navigation helpers; терминальные ветки
дополнительно покрыты сверкой полных модулей и IAB исходных UI-фрагментов.
Синтетические data/callback adapters не означают authenticated product acceptance.
Использован существующий UI pin 430ef57512bca547e44f1a6d4c5878d52ee978ce;
новых API, сборки backend, экспорта SSR или изменения SDK/skills нет.
