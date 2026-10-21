# Подключение общей дизайн-системы

Продукт использует @pdlc/ui 0.3 и опубликованный документ Admin. Общие controls,
цвета, шрифты, плотность, скругления и popup принадлежат Base; запросы,
права, данные и callbacks сохраняются здесь. UI/SDK/skills pins независимы.

Канонический контракт: services-base/docs/platform/DESIGN_SYSTEM.md.
Компоненты искать через pdlc-ui search → show → example. Не создавайте
копии примитивов, собственный Toaster и глобальные определения Base tokens.
Существующие source-компоненты и preview получают общий UI; исторические
прототипы не являются установленным приложением. Установка выполняется отдельно.

Источник UI 0.3.0: `579749e19e9088be200c62e5563a3cb2db4d8c9f` в `.base-ui-revision`.
SDK и skills pins независимы; установка этим source-кандидатом не выполняется.

Vite plugins включает `createUiFontAssetsPlugin()` из `@pdlc/ui/build`: notices
встроенных fonts попадают в dist через общую реализацию Base.
