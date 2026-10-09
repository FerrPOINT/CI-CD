# Общая тема браузера

Потребляется exact Base UI pin из `.base-revision`. Operational header
сохраняет service switcher слева и выбор темы внутри меню аккаунта.
Cookie и cross-origin events принадлежат общему Base primitive; продукт
не копирует их. Backend, auth, roles и данные не меняются.

Контракт: [Base shared theme](https://github.com/FerrPOINT/services-base/blob/19a7a381ae6dbea61a643bb96189e483fa64df5c/docs/plans/2026-10-08-shared-theme-header.md).

Проверки: frontend tests/lint/typecheck/build и Codex IAB на actual bundle,
shared theme в обе стороны, refresh и mobile. Runtime — отдельная веха.
