# ADR-0025: immutable public Cargo seeds для Docker jobs

Статус: принято для source implementation; live activation ожидает native qualification.

Каждый attempt сохраняет собственный writable Cargo registry. Повторно используемые
публичные downloads поступают из operator-owned immutable seed, подключённого
read-only с `nocopy`. Shared writable `target`, source caches, credentials и private
registry не допускаются. Job не публикует и не удаляет seed.

Runner выбирает exact binding по workspace, DB project UUID, actual daemon/image,
SDK revision, lock SHA-256 и platform. Pipeline variables не задают эту identity.
Gateway независимо проверяет тип mount, регистрацию и ownership. Повреждённый
payload даёт диагностированный miss; исходная команда исполняется один раз.
Security denial не заменяется другим mount или Docker endpoint.

Конфигурация выключена по умолчанию. `CICD_RUNNER_CACHE_REGISTRY` и
`CICD_RUNNER_CACHE_REGISTRY_SHA256` задаются доверенным operator вместе, с тем же
registry, который получает Gateway. Нужен immutable tool image с Python 3 для
изолированного bootstrap. Установленные runtime, SDK и skills pins не меняются.

Standalone tooling поставляется из exact Base commit, указанного в
`scripts/vendor/base-forge-cache-tooling.json`. Контракт и обязательная native
матрица: [Base FORGE_CACHE_PROTOCOL](https://github.com/FerrPOINT/services-base/blob/a48ae8d5fecad07675402edff265b2f6c64676f3/docs/FORGE_CACHE_PROTOCOL.md).
Unit tests не заменяют полный runner build и native owner/RO/cleanup proof.

Payload budget входит в общий download pool и считается один раз на seed, а не
на project или attempt. Это admission accounting, не hard disk quota. Никакого
автоматического присвоения старых volumes или общего prune не добавляется.
