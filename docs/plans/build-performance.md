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

## 2026-10-19: отдельная конфигурация frontend units

`frontend/vitest.config.ts` объединяет production `vite.config.ts` через штатный
`mergeConfig`: React/Tailwind plugins, alias и dev proxy остаются у Vite.
Прежние test environment `jsdom`, максимум 4 workers, test/spec glob и
`src/test-setup.ts` сохранены. Обе конфигурации входят в TypeScript проверку.

Изменение только `vitest.config.ts` выбирает frontend unit/static без frontend
compilation в local selector и CI. Production COPY обоих Dockerfiles исключает
этот файл; unit COPY сохраняет его. `vite.config.ts`, production sources,
tsconfig, package/lockfile остаются build inputs. Проверка units по-прежнему
блокирует получение нового image при ошибке, включая прогретую compilation.
Node 22.20.0, pnpm 10.28.1, frozen install, Base UI и OpenAPI handoff не меняются.

Source/selector contracts проверены через RED/GREEN; actual config loading и
TypeScript только двух config files проверены на существующем Node 22.20.0 без
install/build. Cache hits, artifact identity и CPU/RAM новой native сборки этим
source срезом не квалифицированы; они остаются отдельной общей вехой.

Исключён только корневой config: неизвестный `frontend/nested/vitest.config.ts`
остаётся production input. Сам static contract читает оба configs, tsconfig и
два recipes; эти inputs включены в его receipt и tooling component. Изменение
потребляемого файла не получает старый REUSED contract result.

## 2026-10-19: Release без повторного domain suite

Release выбирал combined `backend:unit` и полный `backend:quality`:
domain lib suite и fmt выполнялись дважды при отсутствии unit receipt.
Cargo compilation cache не кэширует исполнение тестов. Standalone protocol
target имеет собственный `[workspace]` и полным `--workspace` не покрывается.

Убрать `release` из combined unit; сохранить его Fast/Unit/Build argv. Добавить
release-only `backend:cache-protocol-unit` с прежней standalone командой и теми
же wrapper/toolchain/SDK/input contracts. Полный quality остаётся владельцем
workspace/domain/doctests/fmt/clippy и прежнего DB gate. Не применять ложный
`covers_units` к external integration gate и не ослаблять Base validator.

Source regressions проверяют owner union, ordinary profiles и separate protocol
workspace. Они входят в existing tooling/CI checks; дешёвый `tooling:build-contracts`
явно выполняется вне heavy host slot. Его receipt покрывает consumed descriptor,
workspace manifests и selector. Native Release/failure/receipt lifecycle и actual
time savings остаются отдельной согласованной вехой без автоматического PG run.

## 2026-10-29: Отдельный слой compile-time delivery witnesses

Оба backend recipes раньше копировали весь `services-base/scripts` прямо перед
Cargo. Изменение постороннего helper становилось входом слоя компиляции.
Новый `delivery-witnesses` на существующем manifests parent экспортирует только
три обязательных файла из `postgres.rs`; `private_acl.py` переносится при наличии.
Прежний `build.rs` проверяет потребность в ACL, а compiled bytes проверяются
прежним privileged owner. Rust-код, SDK pin, dependency/unit stages и runtime
не изменены. Полный candidate key и runtime SDK пока остаются консервативными.

Для SDK `19a7a381` compiler-visible payload — 3 файла / 77 117 B вместо
84 файлов / 953 410 B. Это размер Git blobs на границе compiler input, не
Docker transfer, весь context или RAM приложения. 24 scoped source cases и
16 actual extraction-shell cases PASS, включая missing required inputs,
optional ACL, постороннюю правку и изменение содержимого со старым timestamp.
Native layer hit, binary freshness и CPU/RAM/time проверяются на общей вехе;
сборка ради этого source этапа не запускается. PDLC2 имеет другую transitive
witness closure, PDLC3 не использует этот coordinator; список туда не копировать.
