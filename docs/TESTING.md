# Стратегия тестирования Forge CI/CD — Moved

> **Redirect.** Этот документ объединён в [DEVELOPMENT_GUIDE.md ](DEVELOPMENT_GUIDE.md) и больше не поддерживается здесь.
> Историческая версия доступна в git history. Полная карта консолидации — `docs/DOCUMENTATION_GOVERNANCE.md`; удаление stub-ов запланировано в следующем release-цикле (см. `CHANGELOG.md`).

## Общая база

Подключение версий, границы контрактов и проверки описаны в [BASE_INTEGRATION](BASE_INTEGRATION.md).

## Test-only исходники runner/delivery

Тестовые модули отдельных binaries находятся в `backend/src/bin/tests/`;
production parent сохраняет `#[cfg(test)]` и имя `tests`. Их изменения выбирают
`backend:unit` и static `tooling:bin-test-inputs`, без application producer.
Domain/cache unit не объявляются исполнением bin process tests: их прежний
полный владелец `backend:quality` остаётся на ручной интегрированной/release вехе.
Source command: `python -B -m unittest scripts.tests.test_bin_test_inputs -q`.
Перенос, ограничения и финальные проверки: [план](plans/bin-test-source-boundary-20261029.md).

## Frontend dependency layer

Оба frontend Docker recipes устанавливают зависимости по manifest без root scripts,
сохраняя dependency hooks. OpenAPI и generator копируются после install; затем один
`pnpm run postinstall` готовит schema. Public package commands и runtime tail сохранены.
Source checks не доказывают реальные BuildKit cache hits или ускорение.
Общие правила: Base `docs/BUILD_PERFORMANCE.md`, раздел «Frontend, Python и Java».

Docker install использует зарегистрированный Node22/architecture download store
Base в `/pnpm/store`; integrity check включён, side-effects cache выключен.
`node_modules` остаётся локальным; реальные cache hits и ускорение пока не измерены.

`deployment-path.test.ts` входит в общий `test:unit`: manual `pnpm build`
и Docker unit stage выполняют его перед typecheck/compile. Отдельного Vitest-вызова
этого же файла в umbrella recipe нет; public commands и сам тест сохранены.

## CLI metadata в server Docker recipes

Server image сохраняет `cicd-server` и `forge-runner`; отдельный `cicd-cli`
представлен только Cargo.toml и synthetic main с `compile_error!`. Полный CLI
доступен своему producer и ручным release checks. Изменение production graph,
которое добавляет CLI как зависимость, требует пересмотра этой границы.
Из корня репозитория: `python -B -m unittest scripts.tests.test_ci_contract_checks.ServerCliMetadataTests`.
Existing tooling entrypoint выбирает этот source contract. Native locked/layer/
guard/CLI qualification остаётся на общей финальной вехе; source checks её
не заменяют. Full candidate key/export и SDK/UI/tooling pins этим не сужаются.

Modular SDK в server recipes копируется тремя полными production деревьями
`pdlc-auth-core`, `pdlc-shared`, `pdlc-telemetry`; `pdlc-cli-core` сохраняет только
manifest и lib guard. Source contract: `python -B -m unittest scripts.tests.test_backend_dockerfile.BackendDockerfileTests.test_sdk_copy_matches_production_path_graph_and_preserves_cli_metadata`.
Отдельные CLI/messaging consumers и full SDK export сохраняются; actual layer/
resolver/guard и old-mtime checks относятся к финальной native квалификации.

## Python-входы compiler stage

Оба backend recipes копируют только 67 файлов, объявленных в `BASE_PG_HELPERS`
в `backend/src/task_delivery/postgres.rs`, с прежними путями назначения.
При изменении списка обновить COPY в обоих recipes. Полные SDK export/key,
продуктовые validators, runtime Python и проверки embedded bytes сохраняются.
Source contract: `python -B -m unittest scripts.tests.test_backend_dockerfile -q`.
Он проверяет состав, пути и выбор static owner без application producer.
Actual layer hit, бинарник и ресурсы проверяются на финальной вехе;
правила и evidence — в Base `docs/plans/forge-delivery-witness-layer-20261029.md`.

Docker разделяет полный unit/typecheck context и production COPY по `frontend:build.exclude_inputs`.
Один `pnpm run postinstall` обслуживает оба consumers; `pnpm run build:compile`
зависит от успешного unit-маркера. Native cache-hit/time qualification пока не выполнена.
