# Штатные scoped-проверки Forge PDLC3

**Статус 2026-10-21: Configuration only — source entrypoints; native/hosted qualification отдельно.**

## Контракт и команды

Base владеет выбором и стандартом `docs/BUILD_PERFORMANCE.md` из pinned
`qualityTooling.path` workspace; Forge — фактическими commands/targets.
Из корня workspace: `./check-workspace.ps1 -Profile Unit -Repository CI-CD`;
`-Profile Build -PlanOnly -DetailedPlan` показывает нужные producers.
`-Component tooling -BaseRef HEAD` запускает stdlib route guards без native build.
Standalone selector81467ded поставлен каноническим Base packager; Root quality,
SDK, UI/skills/operator pins не обновляются. UI выделен с прежним SHA19a7a381.

## Что исключено из повседневной работы

Docs выбираются отдельно, в том числе `plans/**`. Frontend не запускает backend,
test/config-only сохраняет unit/static без Vite producer. Неизвестный input
расширяет scope. Production Vite и Vitest configs разделены с прежним inventory,
jsdom/setup/maxWorkers4; test/slow/integration остаются разными tiers.
Domain units: `cargo test --locked -p cicd-domain --lib` без всех test targets.
Один automatic Rust setup/cache обслуживает units и условные service/OpenAPI
checks. Service artifact содержит cicd-server и forge-runner, не openapi-dump
или CLI; весь workspace/CLI остаётся manual release контрактом. Новые artifact
gates cacheable=false до native qualification peer tuple; dependency caches
сохраняются. SDK/source inputs пока консервативны.

Полный исходный CI перенесён в `release.yml` (workflow_dispatch): четыре jobs,
PostgreSQL/CLI/integration, workspace/clippy, MSRV, packed consumer и themes.
Full frontend tests передаются compile-only без второго unit run. Существующие
process backup tests сохранены в manual docs job. Нативные/hosted/live gates
этот source-срез не подтверждает; runtime и данные не меняются.

## Проверка

Stdlib `scripts.tests.test_quality_routes` и `scripts.tests.test_ci_contract_checks`,
canonical packet `--check`, YAML-object comparison с исходным CI и свежий Root
scoped run. Фактические времена route guards — не скорость Cargo или RAM app.
См. [руководство разработки](../docs/DEVELOPMENT_GUIDE.md).
