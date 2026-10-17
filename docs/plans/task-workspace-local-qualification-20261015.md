# Task Workspace Local Qualification

## Текущая Квалификация ce47

Source `ce47d4f8dc473f1e6fd61669444fc149142eb424`, SDK33ce: fmt и strict Clippy
`--locked --workspace --all-targets --features integration --keep-going -- -D warnings`
прошли. Base-v2 project `sdlc-qa-single-task-forge-cfda417ed767` завершился с exit0;
очистка собственных inputs, контейнеров и disposable volumes подтверждена.
Предыдущие отказы Clippy ниже сохраняются как история, не текущий blocker.

Полный frontend этого source, BaseUI8e, Node22.20/pnpm10.28: lint/semantic,
218/218 tests в40 файлах без failures/skips, TSC/Vite build прошли. Отдельный
formatter gate не настроен; whitespace check не выдаётся за formatter.
OpenAPI/client hashes ниже не изменились.

Три PG aggregates прошли на6493. Последующийce47 меняет только тестовый
initializer; production и CLI closure сохраняются. PG batch имел итоговый exit1
из-за прежнего Clippy, и новый отдельный PASS не переписывает его receipt.

На packaging85b реально собран Forge6493 CLI; все8 non-root/offline nested help
paths прошли (вместе с15 Tracker help paths, всего23). Собственные tags и Compose
probes удалены. Итоговый Fleet/Hermes image, установка, backend release image,
Compose smoke и live SDLC/deployment acceptance этим не подтверждаются.

## Follow-Up: 6493 Source

Immutable source6493fe891c39e640bcdf6542212d823463a65ff8 passes all three
remaining PostgreSQL aggregates: original journal/custody/locator/cleanup,
populated0091-to0092 preservation, and actual HTTP-to-embedded-runner Git/shell/
artifact/terminal-ACK/candidate flow. The source delta from qualified5038 is
test-only; generated API and client inputs are unchanged.

Strict all-target Clippy with the integration feature reaches one nonminimal
boolean expression in the new runner fixture. Publishedf78f947 uses the equivalent
Option::is_none_or PID predicate, preserving the same awaited lock, deadline and
ownership checks. This last cosmetic correction is not yet Clippy-qualified;
the original failure remains recorded, not a whole-gate pass.

Project sdlc-qa-single-task-forge-e7e310aff328, own disposable volumes and immutable
inputs are verified cleaned by Base v2. No installed image, protected stand,
deployment, health or acceptance result is inferred from the controlled Tracker
peer and real Git/runner test.

## Earlier 5038 Source

## Qualified Source

Rust source `5038a848ebab056cf917ec86891834dded54df39`, Base SDK33ce and
Rust1.88 are exported from Git into one disposable Base-v2 Compose QA project.
The accepted stand, runtime snapshots and original Hermes are unchanged.

Local passes: production compilation/API export,6 workspace cases,5 trigger/schema
cases,25 preparation cases (including four deadline regressions),2 CLI cases,
one CLI-help case, migration catalog and fresh/prior-history PostgreSQL upgrades.

The journal PostgreSQL aggregate reaches its foreign-principal assertion, then
gets401 instead of403: a second fixture registered its token in another schema.
Published037306a changes only this fixture: an authenticated foreign principal
is registered in the tested router's schema and must still receive403; a separate
unknown token must receive401. The test's remaining original custody/cleanup
assertions and production authorization are unchanged. That correction is not
yet PostgreSQL-qualified. Populated0091-to0092, actual HTTP/embedded-runner and
strict all-target Clippy were not reached by the failed batch.

## Generated API And Client

Actual Rust-generated YAML SHA256:
`168c405687132699400cf98f1b57f150ed9a9ba252f9ea075e19d2ccddca5944`.
Generated TypeScript schema SHA256:
`5848bbd22cd82e9b2978cfbdbdb569e9de459f25aa301071becf452c42a495f0`.

The original compatibility checker passes against main with the corrected object
request schema, without a waiver. Exact frozen Node22.20/pnpm10.28 dependencies
pass generation, independent parity, full TypeScript checking and16 affected
API/hook/pipeline tests with no skipped cases. Locks and BaseUI8e remain unchanged.
The later037306a changes only a PostgreSQL test, not these production/API inputs.

## Remaining And Cleanup

Project `sdlc-qa-single-task-forge-70ae79ccce32` is cleaned through Base v2,
including own disposable volumes and immutable inputs. Evidence is retained.
The original failure remains recorded; generated frontend passes do not replace
backend/PostgreSQL, image, installed-CLI or live seven-role deployment acceptance.
No whole-gate, deployment, health, acceptance or merge-ready result is claimed.
