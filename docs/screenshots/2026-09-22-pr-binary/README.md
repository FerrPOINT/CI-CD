# Бинарный diff pull-запроса

PR detail больше не показывает вымышленные `+0/−0` для полностью бинарного изменения. В строке файла выводится явная подпись «Бинарный файл»; строковые счётчики в сводке отображаются только при наличии текстовых файлов.

## Проверка

- `pr-binary-light-375.png` — mobile, 375×812.
- `pr-binary-light-1920.png` — desktop, 1920×1080.
- Настоящие Central Auth SSO, platform shell и собранный `cicd-web`.
- Список pull-запросов и compare-ответ подменены только в браузере; рабочая БД не изменялась.
- 0 horizontal overflow, serious/critical axe и неожиданных ошибок console/network/runtime.
- Frontend: 151 тест, lint, semantic lint, production build, OpenAPI drift и compatibility.
