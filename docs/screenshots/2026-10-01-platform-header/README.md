# CI/CD PlatformHeader: живые доказательства

Дата: 2026-10-01. Реальные production web image, Central Auth и Admin runtime
catalog в изолированном QA Compose-проекте; API не мокается. Изображения
full-page, а не обрезанный fixture. Все четыре изображения открыты и просмотрены.

- `mobile-dark-375.png`: `/projects`, 375x812, тёмная тема.
- `desktop-light-1920.png`: `/projects`, 1920x1080, светлая тема.
- `wide-gray-2560.png`: `/projects`, 2560x1080, серая тема.
- `services-menu-dark-375.png`: `/settings`, 375x812, непрозрачное runtime-меню;
  ровно шесть healthy UI, текущий CI/CD disabled, API-only и Pulse отсутствуют.

`results.json`: 99 route/theme/width сочетаний; полный Header 60 px, sidebar
под ним, controls 44/40 px, без переполнения/ошибок/записей в продуктовый API.
Keyboard/touch/outside-click/Escape/focus, drawer resize и central logout
проверены `frontend/e2e/platform-header-live.spec.ts`. Serious/critical axe: 0.
Расширенный gate 15/15, retries=0, 7,3 минуты: Header + семь основных страниц +
PR/pipeline/job detail + весь двенадцатитестовый SSO-набор. 291 layout сочетание
(99 + 84 + 108); реальные безопасные jobs, SMTP setup/disable и global logout/PAT.
`metadata.json` связывает проверенный кандидат с Base ref, source/lock и
production JS/CSS/image/config fingerprints. Это не OCI revision attestation
и не подтверждение полного релиза SDLC; пользовательский стенд не обновлялся.
