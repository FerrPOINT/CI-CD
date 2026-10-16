# Проверка backend image build: 16 октября 2026

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
