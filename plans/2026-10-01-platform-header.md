# Общий Header CI/CD

**Статус 2026-10-01:** реализован; локальный no-mock gate 15/15 и frontend 191/191 пройдены.

## Контракт

- Использовать PlatformHeader из services-base main (#119) во всю ширину viewport,
  до sidebar-offset. Leading содержит mobile drawer и ссылку PlatformMark;
  services принадлежит общему Header, actions содержит тему и аккаунт.
- Sidebar начинается под Header (60 px), содержит только семь существующих
  разделов; 72 px на tablet, 264 px от 1280 px. Контекстные PageFrame режимы
  overview PR / inline diff / pipeline / settings не меняются.
- Полная identity видна один раз в непрозрачном меню аккаунта, а не растягивает
  Header. Выход вызывает существующий Central Auth logout; API, local human
  roles и runner/service-account credentials не меняются.
- Mobile drawer закрывается по ссылке, Escape и переходу на desktop breakpoint;
  focus возвращается trigger, body не остаётся заблокированным. Header controls
  44 px на mobile / 40 px на desktop, close drawer 44 px.
- Стандартный каталог остаётся шестью UI и двумя API-only. Pulse не добавляется
  в общий Header и внешний GitHub-репозиторий не создаётся.

## Приёмка

- Unit проверяет slots/ownership, активный раздел, semantic working modes,
  полную длинную identity, закрытие menu и вызов auth logout.
- `frontend/e2e/platform-header-live.spec.ts` работает против реальных CI/CD,
  Admin runtime catalog и Central Auth: 99 route/theme/width сочетаний,
  keyboard/touch/opaque menus, drawer resize, axe и central logout/re-entry.
  Credentials читает только из приватного QA-session файла; API не мокается.
- Повторить семь основных страниц и настоящий PR/pipeline/job сценарий с
  безопасными QA-данными; затем весь SSO-файл на production images.
- Подтверждать image/config/JS/CSS fingerprints и full-page screenshots, не
  выдавать этот срез за окончательную release-приёмку всех продуктов.

Проверено до merge: один непрерывный gate 15/15, retries=0, 7,3 минуты:
99 Header + 84 основных страниц + 108 detail сочетаний; реальные три безопасные
jobs, PR actions, шесть UI/SSO, global logout/PAT и SMTP setup/disable.
Backend fmt/clippy/workspace 146 + реальные PostgreSQL 58 пройдены в отдельной
временной БД. 90 tracked backend source/config файлов checker image сверены
с checkout; backend/API не изменены. Production backend release из того же
неизменённого backend source уже проверен предыдущим locked build; frontend
production image собран заново с frozen install. Full-page кадры и fingerprints
находятся в `docs/screenshots/2026-10-01-platform-header/`.

Локальные gates обязательны. GitHub Actions для этой поставки не используются;
коммит и merge имеют `[skip ci]`. Пользовательский sdlc-demo и его volumes
не изменяются проверкой изолированного QA-проекта.
