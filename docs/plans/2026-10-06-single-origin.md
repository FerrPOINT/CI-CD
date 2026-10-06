# Раздел CI-CD на едином origin PDLC

## План и контракт

Раздел открывается на `/<forge>/` (без угловых скобок), API — под тем же
prefix. Standalone default `/` сохраняется. Внешние порты не служат ссылками
пользовательского переключателя. Base владеет ingress/общим callback и каталогом
правил; продукт сохраняет свои API, identity checks, permissions и навигацию.

`VITE_APP_BASE_PATH=/forge/` задаётся при сборке обоих Docker-вариантов.
Vite `BASE_URL` определяет assets, React Router basename и same-origin API base.
Адрес Central Auth и runtime catalog задаются существующими build args.
SDK pins, API contracts, БД и модель авторизации не меняются.

Проверяются prefixed API requests, compile, deep-link refresh, native SSO,
переключение и повторный вход; сборка/развёртывание выполняются одной общей
вехой Base, не после каждого изменения. Тест использует только фиктивный HTTP
ответ; runtime evidence собирается отдельно на установленном стенде.
