# Namespace DevOps Owner Evidence: 19 October 2026

## Проверенный Source

Source `26b73e0a952685fbdec64f3fa48cf707c84d503d`, SDK
`2ad0834d06d3aa703fa1791a932bfc76650c8e42`. Candidate receipt проверяется как
ReadOnly согласно штатному `TaskWorkspaceIntent::request`, отдельно от writable
рабочего каталога Developer. Отрицательный тест сохраняет корректные остальные
поля и request hash при подмене access.

## Фактический Результат

- 15/15 Linux cases: группы6+7+1+1, без ignored/skips и без подмены списков.
- Fmt, typed lib compile, strict Clippy и Rust OpenAPI generation проходят.
- Исходный runner exit0; Compose и disposable input cleanup подтверждены.
- Original result SHA256:
  `b7c57e280f6025d08a2b2dcedc40f367a59abf8784d3fe6ca1cfac944adad75a`.
- Generated OpenAPI SHA256:
  `268f098310339426e3d9101a217b8ab04638a9f38032d6525a35585e89fb70f7`.
  Снимок скопирован из actual export, не отредактирован вручную. Сравнение YAML:
  один новый путь, пять новых schemas, прежние paths/schemas неизменны;
  дублирующихся operation IDs нет.

Первый harness11cc завершился до Cargo из-за CRLF в generated shell suffix:
0 Rust tests, не product PASS. Его failed evidence сохранён. Отдельный запуск
26b использует explicit LF и проверку исходных байтов перед контейнерным запуском.

## Граница Приёмки

PG-selected0. Проверка R middleware с отсутствующей БД не доказывает успешный
SQL-readback. Production ELF/image, установка, live Auth/R/OCI и Fleet consumer
ещё не приняты. Маршрут не выдаёт полномочий на dispatch и не подменяет Tracker
receipt, текущий lease либо health/acceptance работающего приложения.

## Последующая Квалификация Merge-кандидата

После объединения с main `7b8f98c743434be420947585d64140f82534d7ba`
на Firebat2 проверены текущие backend-входы PR #134:

- Rust 1.88.0: fmt, 372 unit cases и strict release Clippy проходят.
  Пять явно ignored случаев не включены в PASS; два новых PG cases
  дополнительно запущены через `--ignored` и оба проходят.
- Настоящие PostgreSQL17 и Axum reader middleware подтверждают повторяемый
  положительный readback, запрет чужого проекта и неверного assignment,
  отсутствие изменений строк БД и файлов. Размер пула — одно соединение.
- Фикстура проходит штатные DB guards: ciphertext имеет реальный формат,
  original preparation/submission закрепляются при живом embedded lease,
  затем фиксируются completion ACK и успешный pipeline. Исторический child
  не расшифровывается; ключ фикстуры не передаётся reader.
- Endpoint включён в общий `NamespaceReader` inventory. Проверка запрещает
  human Viewer/Admin и сверяет все документированные маршруты с policy.
- Rust OpenAPI экспорт побайтно совпадает с artifact выше. Все прежние paths
  и schemas равны main; добавлены один путь и пять schemas. Operation IDs
  уникальны:189. Клиентские TypeScript-типы обновлены из этого контракта.
- Node22.20.0 / pnpm10.28.1, точный UI pin
  `3865cf4b926bfe52b2e13de1e2c09689f1e80209`: offline frozen install,
  typecheck и `openapi:check` проходят. Внешние версии и lockfile не менялись.

Первый PG-прогон выявил невалидные synthetic credential/lease/receipt rows;
они исправлены, ограничения БД сохранены. Полный unit-прогон выявил пропуск
policy inventory; он устранён в общем владельце маршрутов. Read-only checkout
сохранён; shell-фикстуры получают отдельный временный каталог.

Временный Compose с собственной БД удалён после проверки. Рабочие данные,
runtime, credentials, pins, Hermes и production не менялись. Это квалификация
исходников/API, а не installation, live R/OCI или Fleet acceptance.
Точный опубликованный head и hosted состояние фиксируются в
[PR #134](https://github.com/FerrPOINT/CI-CD/pull/134).
