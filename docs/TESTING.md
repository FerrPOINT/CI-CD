# Стратегия тестирования Forge CI/CD — Moved

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

Обычный CI строит только package `cicd-server`, bins `cicd-server`, `forge-runner`
и `openapi-dump`. Последний используется существующим drift gate без нового
Cargo invocation; build/drift имеют одно условие `backend_build`. Полный ручной
release по-прежнему строит workspace. Source boundary:
`python -m unittest scripts.tests.test_ci_contract_checks.ManualGitReceiveGateTests`;
actual locked targets/OpenAPI требуют native проверки на общей вехе.

Docker SDK profile и его source/shell проверки:
[build performance](plans/build-performance.md#sdk-projection-серверного-образа).
Оба server recipes сохраняют CLI metadata, но блокируют compilation её
placeholder; full CLI/release остаются самостоятельными consumers. Не включать
узкую проекцию старому recipe без metadata target. SDK/operator pins сохраняются.
