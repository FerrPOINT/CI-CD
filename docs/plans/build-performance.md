# Быстрые повторные проверки и сборка

Изменение двух build-contract test files выбирает только static
`tooling:build-contracts`, без frontend/backend builds и dependency preparation.
Gate сохраняет все pure classes этого checkout: Docker contracts, Vite boundary,
CI/manual-receive configuration и CLI configuration. Ordinary CI выполняет их
один раз в existing docs command. `DeliveryPinPreflightTests` с PowerShell/Git
subprocess fixtures остаётся в full module ручного release gate; actual pinned
Base verification и Native QA ownership regressions ordinary CI не меняются.
SDK/quality pins и witness profile сохраняются.

Встроенный `scripts/postgres-delivery.py` входит в backend component и artifact
inputs. Его изменение выбирает backend без frontend и меняет server key даже
при прежних размере/timestamp. `scripts/check_cache_tooling.py` проверяет
product-local literal includes у cache bootstrap и privileged coordinator;
внешние Base helpers сохраняют отдельный pinned SDK contract.

Ordinary backend CI передаёт готовый workspace executable
`${CARGO_TARGET_DIR:-target}/debug/openapi-dump` в прежний OpenAPI drift gate.
Аргумент output path и сравнение YAML сохранены; повторный Cargo graph не нужен.

Применить [стандарт Base](https://github.com/FerrPOINT/services-base/blob/197ead9d9b6cf6901e8f9dd31f43f719885502c3/docs/BUILD_PERFORMANCE.md) без изменения runtime и SDK/UI/skills pins.
Product-owned команды описаны в .quality-gate.json. При разработке/сборке —
unit/static; integration/slow/full coverage — вручную перед релизом.

Для modular Base SDK контекст Forge сохраняет `base-modules/rust`,
`scripts` и транзитивные compile-time witnesses из `base-operator/src` того же
exact SDK pin. Уменьшение контекста не исключает проверки privileged helpers.
Runtime stage сохраняет закреплённый Buildx для обоих источников Compose;
server image компилирует только `cicd-server` и `forge-runner`. CLI остаётся
отдельным consumer и сохраняет полный release gate.

Автоматический backend CI выполняет изолированные domain/cache units без
PostgreSQL. Полный integration/CLI набор находится в ручном `release.yml`.
Сценарий `workflow_dispatch` с `git_receive_only=true` отдельно сохраняет
integration-target clippy, четыре native receive race tests и проверки двух
именованных регрессий. Новый source contract не означает, что эти ручные
проверки выполнены на текущем candidate.

Security/data/compatibility/live gates сохранены; reuse только для допустимых
deterministic exact-candidate проверок.
Проверить no-change, один компонент, dependency/toolchain change и corrupt output;
измерить duration и cache hit/miss. Сохранять прогретые caches и чистить own scratch.

Docker frontend: раздельные unit/compile stages, unit/static перед compilation;
TypeScript проверяется один раз в обязательном unit stage; compile запускает Vite.
production COPY исключает тесты. Frozen dependencies сохраняют общий совместимый
pnpm download store. Parent digests закреплены; Dockerfile frontend, общий RUN slot и
ограничения Windows producers определяет стандарт Base.
Runtime и package build scripts сохраняют прежние публичные команды.

Frontend CI handoff: один `dist` с manifest/digests, затем verified nginx packaging.
Команда self-check и product flags: [DEVELOPMENT_GUIDE](../DEVELOPMENT_GUIDE.md#передача-frontend-artifact). SDK/UI/skills pins сохраняются.

Shell unit работает с реальным QueryClientProvider и отдельным fresh
workspace-catalog cache без сети. Native qualification проверяет false/true:
Namespace context добавляет header slot только в enabled mode. Production
source, SDK/UI pins и release набор не меняются.

Отдельные test setup исключены из production COPY
и `frontend:build` inputs, но остаются входами unit/static. Только эти явно
объявленные пути допускают пропуск CI compilation; общий `vite.config.ts`
с aliases/plugins остаётся полноценным build input. Ошибка unit блокирует
получение нового образа даже при сохранённой production compilation.

Docker installation получает manifest без `scripts` через отдельный cheap
BuildKit stage и content-addressed COPY. Установка выполняется с
`--frozen-lockfile --ignore-scripts`, сохраняет lock/workspace и настоящий Base
package. Полный product manifest возвращается перед проверками и compilation.
OpenAPI postinstall выполняется один раз в отдельном generated stage; spec,
generator и полный manifest определяют его cache key. Непустые outputs
проверяются и копируются после frontend sources в unit/application stages.
Изменение scripts/spec не запускает повторный install; отсутствие output или
ошибка generation блокируют результат, включая прогретый compile cache.

CI Rust target после restore готовится через `python3 ../scripts/vendor/base_rust_target_cache.py` из `backend/`; версия и checksum включены в отдельный Base quality packet.

## SDK projection серверного образа

`candidate_sdk_projection: forge-runtime-v1` в `.quality-gate.json` объявляет
совместимость Dockerfile и Dockerfile.umbrella с узкой проекцией нового Base
producer. Полный код копируется для pdlc-auth-core/shared/telemetry; от cli-core
нужен только Cargo.toml, поскольку CLI остаётся необранным членом workspace.
Recipe создаёт metadata target с `compile_error!`: случайная попытка скомпилировать
его завершается ошибкой вместо поставки пустой библиотеки. Реальный CLI/Fleet
producer продолжает получать настоящий SDK, без этого placeholder.

Dependency и final stages обоих recipes выбирают только server/runner binaries.
Pinned delivery helpers, operator witnesses, migrations, units и runtime tools
сохранены. Старый Base producer может передавать больший SDK context; новый
сужает его только при явном profile. Неизвестный profile/layout отклоняется.

Scoped regressions: `python -B -m unittest scripts.tests.test_backend_dockerfile -q`.
Shell fixture исполняет dependency command с recording Cargo и проверяет target
до вызова compiler stub. Actual locked image/build, old-mtime SDK invalidation,
unused input cache hit и artifacts проверяются на общей native вехе; source
checks и размер COPY segment не являются измерением CPU/RAM или скорости.

### Точные compiler witnesses

Build stage обоих recipes копирует ровно 66 файлов из `BASE_PG_HELPERS`,
сохраняя прежние include paths. Изменение постороннего Python helper не должно
сбрасывать этот Docker layer. Regression сравнивает полные множества COPY и
embeds; проверка только наличия необходимых файлов недостаточна.
Macro использует `::std::include_bytes!`/`::std::concat!`, исключая shadow
импортированных macros. Это source contract; actual locked compilation,
old-mtime binary change, layer hit и PostgreSQL delivery ещё требуют native gate.

В Base реализован дополнительный `forge-runtime-witness-v1`: closed parser
фиксирует inventory в dependency request и применяет его к view/fingerprint.
Полный SDK export/runtime package сохраняет дополнительные imports.
Текущий descriptor оставляет `forge-runtime-v1`, совместимый с принятыми callers.
Новый profile включается отдельно после qualification и принятия exact tooling
с parser; автоматически изменять SDK/quality pins или объявлять reuse нельзя.

Test options теперь живут в отдельном `frontend/vitest.config.ts`, наследуя
production config. Его изменение выбирает проверки без app artifact, и оба
production COPY исключают этот файл. Types/config loader проверены отдельно от
native app build; [границы проверки](../TESTING.md#раздельная-vitevitest-configuration).

Pure execution-filter tests переведены в Node, DOM fixtures/imports стали
условными. Три same-case samples и соседние guards/QueryState прошли;
[измерения и границы](../TESTING.md#pure-node-suites-и-browser-setup).
Actual source-key controls подтверждают прежний app key, новый unit key и
выбор только types/tests. Production config/dependencies и runtime не менялись.
