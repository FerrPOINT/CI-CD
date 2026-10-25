# Стратегия тестирования Forge CI/CD — Moved

## Python helper unit

Известные pure helper test paths перечислены в `tooling:python-helpers`:
build-pin, backup dry-run, frontend toolchain, observability/TLS/docs,
OCI/PostgreSQL control-flow models и routing. Fast/Unit/Build не выбирают из-за
них application producers. Существующие build-contract tests сохраняют свой
`tooling:build-contracts`; unknown scripts сохраняют широкий fallback.
Root preview: `check-workspace.ps1 -Profile Unit -Repository CI-CD -Component tooling -PlanOnly`.
Один module из Forge: `python -B -m unittest scripts.tests.test_postgres_delivery -q`.
Полная helper command определяется argv `tooling:python-helpers`; CI объединяет
её с build-contract modules без повторов и сохраняет actual docs/SBOM validators.

Два OCI retention/confinement cases требуют POSIX modes, symlink и O_DIRECTORY.
На Windows они явно SKIPPED; их неизменные bodies выполняются Linux CI и должны
быть проверены на Linux до заявления полной qualification. Windows source proof
не заменяет их. `test_native_qa_compose` с explicit SDLC_MAINTENANCE_BASE и
POSIX stand-backup fixtures этим pure gate не активируются; их qualification
остаётся отдельной. Реальные Docker/PostgreSQL/SDK live gates не ослаблены.
Отрицательные README fixtures создаются в TemporaryDirectory и не переписывают
checkout. Результаты — [source план](plans/python-tooling-test-selection-20261024.md).

Актуальная политика 2026-10-15: unit/static запускаются при обычной сборке;
integration/slow/full coverage и browser/live — вручную перед релизом.
Команды быстрых уровней приведены в разделе «Тестовые уровни (2026-10-15)» ниже.
Прежняя полная матрица и тяжёлые команды этого документа сохранены как
ручные release checks; они не запускаются автоматически после каждой правки.

> **Redirect.** Этот документ объединён в [DEVELOPMENT_GUIDE.md ](DEVELOPMENT_GUIDE.md) и больше не поддерживается здесь.
> Историческая версия доступна в git history. Полная карта консолидации — `docs/DOCUMENTATION_GOVERNANCE.md`; удаление stub-ов запланировано в следующем release-цикле (см. `CHANGELOG.md`).

## Общая база

Подключение версий, границы контрактов и проверки описаны в [BASE_INTEGRATION](BASE_INTEGRATION.md).

## Тестовые уровни (2026-10-15)

Обязательная общая политика: [Base TESTING](https://github.com/FerrPOINT/services-base/blob/ad724088e9a63b1e353fb5b28e4308e9ae6f45ae/docs/TESTING.md).
Выбор компонентов и native commands: `.quality-gate.json`.

### Разработка и сборка

Backend: `cargo test --locked -p cicd-domain --lib` из backend/.
Cache protocol: `cargo test --locked --manifest-path tests/cache-protocol/Cargo.toml --lib`
из backend/. Этот маленький target проверяет production `runner_cache.rs` напрямую,
без компиляции HTTP/PostgreSQL server. `python scripts/check_cache_tooling.py`
проверяет exact Base packet и совпадение dependency versions с application lockfile.
Оба unit target обязательны в обычной сборке; native Docker boundary qualification
выполняется отдельно перед активацией по [ADR-0025](adr/0025-readonly-cargo-seeds.md).

Frontend из frontend/: `pnpm test:unit`, `pnpm build` включает unit.
`pnpm test` — unit alias, `pnpm test:all` — прежний полный набор.
Playwright, packed-consumer и effective-theme проверки — ручной release.

`frontend:types` и `frontend:tests` явно принимают Base contract
`frontend-agent-presence-v1`: непустые correlation ID агента имеют presence
семантику в фактической среде команды. Остальные inputs, пустые/отсутствующие ID,
Node/pnpm и VITE settings сохраняются. Область фиксирует
`FrontendToolchainTests`; lint/production artifact этим opt-in не охвачены.
Это не разрешение повторно готовить dependencies при неизменном install key.

### Ручная релизная проверка

Полный прежний набор сохранён в `.github/workflows/release.yml`: только
workflow_dispatch на exact ref. Автоматический CI не поднимает PostgreSQL
и не запускает длительные/browser/live тесты.

Required security/data/compatibility checks и thresholds сохраняются.
Unit success не заменяет ручную приёмку; NOT RUN не означает PASS.

Cache bootstrap включается в server через `include_str!`. После обновления
`scripts/vendor/base-forge-cache-tooling.json` нужен новый server artifact;
одних Python tests недостаточно. Native qualification выполняется отдельно,
под UID 10001 с `deploy/forge-job-seccomp.json`, без изменения runtime/pins.
Текущий packet использует разрешённый directory-relative `linkat` и сохраняет
атомарность/no-overwrite; sandbox ради cache hit не расширяется.
`check_cache_tooling.py` также сверяет реальные `include_str!` с backend scope и
artifact inputs. Unit: `python -B -m unittest discover -s scripts/tests -p test_cache_tooling.py -q`.
Изменение cache packet обязано менять native artifact identity и не выбирает
frontend. Неизвестные остальные изменения по-прежнему расширяют проверку продукта.

## Одна OpenAPI generation в CI

В frontend job автоматического CI и ручного release `postinstall` выполняет
generation во время frozen install; project lifecycle явно включён. Сразу после
установки Git проверяет tracked outputs и отсутствие drift, без второго
generator invocation. Проверяемые outputs: `src/api/schema.d.ts`.
Lockfile, backward compatibility и backend OpenAPI drift gates сохраняются.
Самостоятельный `pnpm openapi:check` остаётся проверкой для окружения, в котором
generation ещё не выполнялась; его не добавляют повторно после postinstall.

## Точные Rust artifacts обычной сборки

`backend:build` в `.quality-gate.json` выбирает только package/bin targets
поставляемого runtime: `cicd-server`, `forge-runner`.
Outputs именованы явно; отсутствие любого required executable завершает gate
ошибкой. Случайный соседний executable не попадает в artifact.
Standalone CLI, delivery/migration tools и OpenAPI не включаются в server artifact; их отдельные consumers и полный ручной release остаются.
`backend:quality` сохраняет полные workspace clippy/tests в ручном release;
MSRV/features/OpenAPI и необходимые integration gates не сокращаются.
Fresh target между source exports и registry/compiler-cache policy Base сохраняются.

Root quality Release выбирает полный `backend:quality` и отдельный
`backend:cache-protocol-unit`. Domain lib suite и fmt выполняются внутри полного
quality один раз; standalone protocol workspace сохраняет собственный запуск.
Fast/Unit/Build по-прежнему выбирают прежний combined `backend:unit`.
Source routing contract: `python -B -m unittest scripts.tests.test_backend_release_ownership`.

## Минимальная подготовка Docker unit target

В двух backend recipes unit имеет ancestry `manifests → unit-dependencies → unit`.
Запрос `--target unit` готовит только `cicd-domain --lib` и отдельный
`tests/cache-protocol --lib`; предварительная компиляция server/runner в этот
путь не входит. Полная сборка расширяет тот же immutable dependency layer в
`dependencies`, затем `build` требует исходный unit marker. Все реальные unit
команды, поставляемые binaries, runtime и pinned toolchains сохранены.

Source guard принадлежит единственному `tooling:build-contracts`:
`./check-workspace.ps1 -Profile Unit -Repository CI-CD -Component tooling`
из корня соответствующего workspace. Обе recipe входят в component и inputs.
Source checks не заменяют native qualification нового recipe и измерения
приложения. Общий контракт — [Base BUILD_PERFORMANCE](https://github.com/FerrPOINT/services-base/blob/4eadd6bd469e584e26da066f36026d3e0c354378/docs/BUILD_PERFORMANCE.md), выбранный по
`qualityTooling.path` корневого inventory. Product docs не требуют копировать
новый source Base или менять независимый SDK pin.

## Точный маршрут documentation validator

`scripts/verify_docs.py` и `scripts/tests/test_verify_docs.py` выбирают `tooling:python-helpers`.
Fast для source validator также сохраняет `docs:contracts`; Unit/Build не выбирают application producers.
Source command: `python -B -m unittest scripts.tests.test_verify_docs`.
Mixed backend и неизвестные source paths сохраняют своих consumers; actual CI
и ручные release gates остаются прежними.
