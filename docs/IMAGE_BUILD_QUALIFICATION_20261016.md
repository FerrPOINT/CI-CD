# Проверка backend image build: 16 октября 2026

## Текущий Кандидат d3a0bbe

Исходник `d3a0bbef79764587f21ad819f412d1e0f73fa95e`, SDK7c7b24c:
закрытый локальный QA завершился с exit0 и verified cleanup.
Проходят fmt, metadata, компиляция lib и integration target с `oci-integration`,
34 целевых unit-теста, atomic-terminal PG1 (19.75s), прежний настоящий HTTP
shell regression1 (1.94s), Rust OpenAPI export и строгий workspace/all-target
Clippy с `oci-integration`. Это не полный workspace test suite.

Скомпилирован, но не запущен opt-in native aggregate
`task_pipeline_http::image_build_native::task_http_backend_image_build_tls_artifact_oci_health_and_acceptance`.
SHA256 сохранённого integration executable:
`7609be921319628ca02fbe6577e0d5edf405644830e4e55e40e7ce51a14bab2a`.
Бинарник сам по себе не доказывает native build/deployment.

SHA256 текущего QA result.json:
`845212ff9a817f8ebe9d4dcf9af52d293e3b02471d0f04cae8d84f0e8bce823c`.
SHA256 group-results:
`691f58238f4a65e8f67348f325935e29d98afe22de0e18b031914ab8649c8030`.
Временные ресурсы этого QA удалены; source inputs также убраны.

Отдельно существующий Base44 Build собрал backend-кандидат из exact d3a0bbe
и SDK7c, с сохранённым consumed-source receipt. Image ID:
`sha256:b3eedae7fd06ad81ae91ebd88b2e0cde5027325c75cf21a39cc53fe8bb3beadb`.
SHA256 build receipt:
`41cd4e1777b9a095164f1a5ec8dc2d418096626388a0db5a8c6914d9936abe29`.
Образ имеет non-root user `cicd`; Buildx0.37.2 checksum/version проверены при
упаковке. `complete=false` относится к partial workspace build из одного сервиса.

Этот backend image не установлен и не является образом результата задачи.
Task HTTP -> image build на configured TLS daemon -> artifact -> OCI deployment
-> health/acceptance ещё не выполнен. Реальная placement/data mapping должна
быть проверена; одинаковое имя volume на разных daemon не доказывает общие bytes.
Прежний PG500 остаётся неклассифицированным: два последующих PASS подтверждают
воспроизводимость на этих inputs, а не установленную причину старого отказа.

## Предыдущая Проверка aaad

## Подтверждено

Локальный временный Compose проверял исходник
`aaad547e46a7f29bf5a11335fcf25a9f77cfa839` с Base SDK
`7c7b24cd57cb4470b696d97933283018a20fbbb3`.
Принятый стенд, его данные и Docker credentials не изменялись.

- Rust fmt, metadata и компиляция server lib с integration feature проходят.
- 34 целевых unit-теста проходят: domain3, primitive12, Docker client3,
  parser3, admission9, namespace closure1 и OCI transport3.
- PostgreSQL aggregate атомарного завершения: 1 passed, 0 failed/ignored,
  22.18 секунды. Проверяются реальные блокировки, expiry reconciliation,
  cancel/stale fence, rollback при отсутствующей queue и истечении deadline.
- Rust OpenAPI export выполнен. Ответ task delivery включает типизированный
  OCI readback, а не только прежний static readback.
- Опубликованная OpenAPI-схема синхронизирована с этим экспортом; клиент
  сгенерирован через Node22.20/openapi-typescript7.13 с проверенным lockfile
  dependency closure. Строгая проверка деклараций TypeScript5.9.3 проходит.
  Это не новый полный frontend test/build gate.
- Временные контейнеры, сети и disposable volumes удалены;
  `cleanup_verified=true`, журнал v2 имеет состояние `cleaned`.

SHA256 фактического result.json:
`0697a0259f2633073a2e13f1d081dd9b679bcd59fad1125d4111bb7b97f2820a`.
SHA256 group-results.txt:
`4dc53f5cf168123fd30a9d8e6e9f80f627fd73aff1adde30459dbaa68729cde0`.
SHA256 PostgreSQL log:
`0945ab34d9050aebeb76434b98aa060f3713ddbaaaa2825fedce6c1bcbeccf60`.
Секреты, полный metadata JSON и приватные логи не публикуются.

## Не Подтверждено

Итоговый runner exit равен 1: all-target Clippy обнаружил два оставшихся
импорта `sdlc_shared` в test fixtures при фактическом SDK `pdlc_shared`.
Commit `f69b5be8db6586188a9eac5653e807fd06803fe9` исправляет только эти
импорты; его all-target проверка ещё не выполнялась. Это не workspace PASS.

Предшествующий исходник `49b6286072980b30f59758a99b4caea8458c3dc3`
получил неклассифицированный HTTP500 в PostgreSQL aggregate. На aaad тест
проходит, но его SQL/assertions/timeouts не менялись. Следовательно,
причина прежнего отказа не установлена; успешный повтор не объявляется fix.

Настоящие Buildx/TLS build, artifact publication, HTTP OCI deployment,
health/acceptance и original-key readback должны пройти отдельную native
проверку. Unit-тесты и синтетический Tracker peer не доказывают полный
семиролевой SDLC, установку кандидата или работу модели.

Hermes остаётся оригинальным. Docker authority принадлежит только backend;
агенты и jobs не получают socket, TLS credentials или Docker endpoint.
