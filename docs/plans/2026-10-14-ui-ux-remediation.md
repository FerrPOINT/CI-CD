# Доработки UI/UX 2026-10-14

UX-06/08. Контекст PageHeader объясняет показатели подключённых проектов, добавлены ссылки на репозитории и CI-конфигурации. SQL и непривязанные ресурсы сохранены. Вход и callback SSO используют Base.

Общие контракты и тексты: [стандарт Base](https://github.com/FerrPOINT/services-base/blob/6b9dd35a92bb095debf166724e4ecb1cb0e8ceb0/docs/platform/UI_UX_STANDARD.md). UI pin: `6b9dd35a92bb095debf166724e4ecb1cb0e8ceb0`; SDK и skills pins сохранены.

Frontend: 218 тестов; TypeScript/lint, build, frozen lockfile и OpenAPI. IAB: общий анонимный вход на 320/375/1920/2560.

Зависимость: [Base PR #206](https://github.com/FerrPOINT/services-base/pull/206). Кандидат поставляется draft: полная межпродуктовая IAB-приёмка, SSO и 30 направлений переключения ещё не подтверждены. Backend не менялся; его полный локальный Rust gate был остановлен после ограничения на дополнительные Docker-группы. GitHub CI учитывается отдельно; отсутствие запуска из-за billing не объявляется PASS.

Работа и native QA выполняются в PDLC2 без новых Docker-групп. Merge и установка исключены; установка — отдельная задача.

## Генерализация UI

InitialsAvatar, ConfirmDialog, PageNavigation, useDebouncedValue и общий QueryState используются из Base. Локальный QueryState содержит только правило показа ForbiddenPage.

## Общие блоки текста и описаний

DiffBlock заменяет два одинаковых PatchView. Forge передаёт patch, accessible name и сообщение об усечении; цвета/прокрутка/строки принадлежат Base.
