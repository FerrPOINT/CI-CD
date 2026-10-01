# CI/CD detail: локальная live-приёмка

1 октября 2026, изолированный production Compose-проект, настоящие Central
Auth/CI/CD API и внутренний Git. Никаких API mocks или внешних репозиториев.

Один непрерывный Chromium-прогон: **2/2**, retries=0, 2,6 минуты.
108 detail + 84 основных route/theme/viewport сочетания. Проверены реальная
геометрия rail/stage columns, три безопасные printf jobs и логи, PR/diff,
обе подтверждающие формы на 375/2560 px, touch/keyboard, Escape/focus return,
контраст и высота кнопок не меньше 40 px. Ноль page overflow, console/network
errors, serious/critical axe и UI-мутаций при detail-навигации.

Full-page кадры выбранного прогона:

- [PR, light, breakpoint 1024](pull-light-1024.png).
- [PR, dark, desktop 1920](pull-dark-1920.png).
- [Diff, gray, desktop 2560](diff-gray-2560.png).
- [Pipeline logs, dark, mobile 375](logs-dark-375.png).

[Результаты и fingerprints](results.json),
[план и route mapping](../../../plans/2026-10-01-detail-layout-modes.md),
[команда запуска](../../UI_SHELL.md).

QA удаляет только собственные project/repository через штатные API. PR/audit
history без DELETE API остаётся в изолированной базе до удаления QA-проекта.
Пользовательские volumes не затронуты. Snapshot не является OCI revision
attestation; пользовательский стенд не перевыкатывался, финальный платформенный
release gate остаётся обязательным. Секреты/traces не публикуются.
