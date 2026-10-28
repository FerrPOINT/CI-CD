# Forge: test-only исходники отдельных binaries

**Статус 2026-10-29:** source реализован; static PASS; native/fmt/Docker NOT RUN.

Цель: изменение теста runner/delivery не выбирает production artifact и не
инвалидирует Docker COPY production sources. Используются существующие
backend:unit и backend:build; новый static contract проверяет границу inputs.

## Исходники и порядок

1. Вынести завершающий `#[cfg(test)] mod tests` из `forge-runner.rs` в
   `src/bin/tests/forge_runner.rs`; при наличии delivery — аналогично в
   `src/bin/tests/forge_delivery.rs`. Parent сохраняет `cfg(test)`, имя `tests`
   и явный path. Проверить одинаковые Rust tokens тела, включая string literals,
   и неизменный production prefix. Targets/dependencies/lockfiles не менять.
2. Production COPY обоих Dockerfiles исключает только эти файлы; обычный
   checkout и прежний full workspace/release test graph сохраняют модули.
   Dockerfile frontend закрепить на уже принятом в PDLC1 1.27.1 digest,
   который поддерживает COPY exclusions. Другие FROM/toolchain pins прежние.
3. Сохранить artifact input exclusions и выбрать fast static contract для
   изменённых declarations/fixtures/recipes. Не включать cacheable=true там,
   где он ещё не квалифицирован. Production neighbor обязан менять artifact key.
4. Scoped Python contracts и source fingerprint controls; compiled tests,
   rustfmt и Docker cache/invalidation — на согласованной финальной вехе.

## Ограничения

В текущем Windows source окружении Rust/rustfmt недоступны; не готовить среду
ради проверки и не выдавать Python lexer за rustc. Native/fmt результат остаётся
NOT RUN до финального gate. Bin process tests сохраняют прежний manual
workspace/release owner; перенос файлов не запускает их автоматически и не
снижает release inventory. В PDLC3 delivery может отсутствовать: не добавлять
несуществующий target или тесты из соседнего workspace. Runtime не изменять.

Общие правила: [Base BUILD_PERFORMANCE](https://github.com/FerrPOINT/services-base/blob/00aa287377e4b390e336af78e63e63a5f62b0f37/docs/BUILD_PERFORMANCE.md).

PDLC2 сохраняет отдельного владельца `delivery:build`; новый test-only path
относится к прежнему delivery component и исключается из соответствующего
artifact. В остальных workspace сохраняются их фактические владельцы.

## Проверено в source, 29 октября

Scoped Python contracts: PDLC1 — 28, PDLC2 — 51, PDLC3 — 37 PASS.
Actual frozen fingerprint implementation каждого workspace проверен на маленьком
Git fixture: test-only key artifact прежний, manual quality key изменён,
production neighbor меняет artifact key. Это 6/6/3 controls соответственно;
artifact execution/reuse этим не доказываются. Rust test tokens и production
prefix сравнены до/после, включая literals; это не native compile proof.

PDLC3 validator paths сохраняют точного прежнего static owner через существующую
`affected_checks` карту; unknown/mixed source paths сохраняют component fallback.
Native Rust tests, rustfmt, Docker invalidation, длительность/CPU/RAM приложения —
NOT RUN/NOT MEASURED. Сборки и подготовка среды на этом этапе не выполнялись.
