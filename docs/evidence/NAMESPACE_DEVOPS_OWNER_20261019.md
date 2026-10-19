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
