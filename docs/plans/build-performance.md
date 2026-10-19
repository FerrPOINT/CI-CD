# Быстрые повторные проверки и сборка

Ordinary backend CI передаёт готовый workspace executable
`${CARGO_TARGET_DIR:-target}/debug/openapi-dump` в прежний OpenAPI drift gate.
Аргумент output path и сравнение YAML сохранены; повторный Cargo graph не нужен.

Обычный CI выбирает только `cicd-server`, `forge-runner` и `openapi-dump`.
Оба production Docker recipes выбирают два поставляемых runtime binaries,
включая dependency stage; manual/release workspace checks сохраняются отдельно.

Встроенный `scripts/postgres-delivery.py` входит в backend component и artifact
inputs. Его изменение выбирает backend без frontend и меняет server key даже
при прежних размере/timestamp. `scripts/check_cache_tooling.py` проверяет
product-local literal includes у cache bootstrap и privileged coordinator;
внешние Base helpers сохраняют отдельный pinned SDK contract.

Правки `test_backend_dockerfile.py`/`test_ci_contract_checks.py` выбирают один
`tooling:build-contracts` без frontend/backend builds и без подготовки окружения.
Это чистые script/config checks; они также входят в обычный CI script step.
Неизвестные paths сохраняют conservative fallback, реальные backend/frontend
inputs по-прежнему выбирают свои producers. SDK и quality pin не меняются.

Применить [стандарт Base](https://github.com/FerrPOINT/services-base/blob/197ead9d9b6cf6901e8f9dd31f43f719885502c3/docs/BUILD_PERFORMANCE.md) без изменения runtime и SDK/UI/skills pins.
Product-owned команды описаны в .quality-gate.json. При разработке/сборке —
unit/static; integration/slow/full coverage — вручную перед релизом.
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
