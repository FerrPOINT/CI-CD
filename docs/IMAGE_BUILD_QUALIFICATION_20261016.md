# Проверка backend image build: 16 октября 2026

## Текущее Исправление 7cf6afc

Native aggregate реально запущен на исходнике67 с сохранённым binary a7f632
в non-root backend D04 и отдельном TLS engine. Buildx создал образ; журналы
содержат финальные export/unpack DONE. Но pipeline terminal receipt не получен:
тест завершился101 на проверке known terminal custody, до OCI deployment.
Это не успешный native build/deployment gate.

Подтверждена конкретная несовместимость Docker29: `Config.Volumes` отсутствует
у собранного image, поэтому прежний Go template `.Config.Volumes` завершается
ошибкой. Read-only вызов из того же backend с `index .Config "Volumes"` возвращает
null и совпадающие image/revision/attempt/request/OS. Никакой повтор build или
ручной terminal ACK при диагностике не выполнялся.

Исправление `7cf6afc427eb75f1e1af27aee725a54ce588f745` меняет только чтение
необязательного поля и добавляет регрессию. Проверки exact image/source/attempt/
request/daemon и запрет непустых volumes сохранены. Rustfmt/diff PASS; новая
компиляция, unit/native и runtime image qualification ещё не выполнены.
Исходник Hermes, CLI и публичные API/DTO не меняются.

Private evidence находится под
`.local/single-task-sdlc-20261013/forge-native-inputs-20261016/evidence-8c94daa5a999414d-staged/`.
SHA256 redacted native diagnostic0024:
`3e75aeda6d71f35dafe5f9056d89b00db7fbdb84806751b8834453c31a7af2d2`.
После отказа scopes сохранены для original readback. Затем Main явно прекратил
synthetic QA: private snapshots workspaces/data/remote volumes и PG dump
сохранены и проверены по SHA256. Только его ресурсы удалены после проверки v2
owner labels, manifest hash, daemon и отсутствия foreign refs, без force.
Образ и evidence сохранены. Held-журналы не переписаны в SDK `cleaned`,
продуктовый ACK не создан; отдельный manual cleanup receipt имеет SHA256
`36dbd716f3a567418ce72cfa45c2ae0d54a3fc0b79d2f6714eb52a198db8047c`.
Это не native PASS или бизнес-завершение. Отсутствие deployment receipt
не заменяется фактом существования image.

## Предыдущая Квалификация 67fe990

Исходник `67fe990a0de34f42796241a0fcb6cacb61f6588d`, SDK7c7b24c:
отдельный локальный QA закрыт с exit0, journal `cleaned`, verified cleanup.
Независимый Docker readback не обнаружил собственных контейнеров или volumes;
disposable source inputs также удалены.

Проходят fmt, metadata, lib/integration compile с `oci-integration`, прежние34
unit-теста и новые probe10/transport7. Их exact listing/name inventory проверен:
policy test учитывается в собственном `policy_tests`, а не теряется в фильтре.
Atomic-terminal PG1 проходит за11.51s, прежний HTTP shell1 за1.78s; Rust OpenAPI
export и strict workspace/all-target Clippy с `oci-integration` проходят.
Это не полный workspace test suite и не native task deployment.

```text
QA result:   7dbd51e2864cd5f543cdce7ec02fd04e4a8e7e4fb2d0dd15eb8af065d7416e18
group ledger: 0fbef175787a99269e107a4e21057c159013530310f703b3902e2a97f00d3b44
native binary: a7f6324061c7cb67ad4be768df513eb3c027dc09861ab2360edd81a2dabc7dad
```

На момент этого отдельного compile receipt бинарник был только скомпилирован.
Последующий неуспешный native запуск описан выше и не изменяет старый receipt.
Контролируемый Tracker peer не является live evidence семи профессий.

Отдельный штатный Base44 Build из exact67fe990/SDK7c создал backend-кандидат:

```text
image: sha256:d04ba18a729ab48702a284ad5ac3478fa6bdb7604e18310cb63d414e0cc61130
source archive: 0517cf971d7b4d8c00412f3f883d807be2c2fd85c88a2d8e7e97c7d6f49e23fc
build receipt: eaec24bf4006d95baaa47031251d2847ef51d99ad5db29ee500a1d9af2e3ea3c
```

`complete=false` означает сборку одного сервиса, а не отказ. Образ не установлен
и не является image результата задачи. Фактический task TLS build, передача
snapshot между engines, native HTTP OCI/health/acceptance и полный live SDLC
остаются открытыми. Неизменные wire/API/client сохраняют отдельную codegen
проверку; новая публичная DTO-схема этим пакетом не вводится.

## Предыдущий Кандидат d3a0bbe

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
