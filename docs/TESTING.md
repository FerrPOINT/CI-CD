# Стратегия тестирования Forge CI/CD — Moved

Frontend manifest/importer больше не объявляет неиспользуемый
`i18next-http-backend`: product и Base переводы уже bundled JSON. Configs,
locales, scripts, locked versions и pins сохранены. Новый lock требует отдельной
frozen qualification; старый environment не считается проверенным этим Source audit.
[Общий consumer contract и результаты](../../services-base/docs/plans/fleet-frontend-dependency-closure-20261030.md).

> **Redirect.** Этот документ объединён в [DEVELOPMENT_GUIDE.md ](DEVELOPMENT_GUIDE.md) и больше не поддерживается здесь.
> Историческая версия доступна в git history. Полная карта консолидации — `docs/DOCUMENTATION_GOVERNANCE.md`; удаление stub-ов запланировано в следующем release-цикле (см. `CHANGELOG.md`).

## Общая база

Подключение версий, границы контрактов и проверки описаны в [BASE_INTEGRATION](BASE_INTEGRATION.md).

## Точный маршрут documentation validator

`scripts/verify_docs.py` и `scripts/tests/test_verify_docs.py` выбирают `tooling:quality-routes`.
Fast для source validator также сохраняет `docs:contracts`; Unit/Build не выбирают application producers.
Source command: `python -B -m unittest scripts.tests.test_verify_docs`.
Mixed backend и неизвестные source paths сохраняют своих consumers; actual CI
и ручные release gates остаются прежними.

## Test-only исходники runner/delivery

Тестовые модули отдельных binaries находятся в `backend/src/bin/tests/`;
production parent сохраняет `#[cfg(test)]` и имя `tests`. Их изменения выбирают
`backend:unit` и static `tooling:bin-test-inputs`, без application producer.
Domain/cache unit не объявляются исполнением bin process tests: их прежний
полный владелец `backend:quality` остаётся на ручной интегрированной/release вехе.
Source command: `python -B -m unittest scripts.tests.test_bin_test_inputs -q`.
Перенос, ограничения и финальные проверки: [план](plans/bin-test-source-boundary-20261029.md).

## CLI metadata в server Docker recipes

Server image сохраняет `cicd-server` и `forge-runner`; отдельный `cicd-cli`
представлен только Cargo.toml и synthetic main с `compile_error!`. Полный CLI
доступен своему producer и ручным release checks. Изменение production graph,
которое добавляет CLI как зависимость, требует пересмотра этой границы.
Из корня репозитория: `python -B -m unittest scripts.tests.test_ci_contract_checks.ServerCliMetadataTests`.
Existing tooling entrypoint выбирает этот source contract. Native locked/layer/
guard/CLI qualification остаётся на общей финальной вехе; source checks её
не заменяют. Full candidate key/export и SDK/UI/tooling pins этим не сужаются.

SDK CLI также представлен manifest и lib guard; полные `sdlc-auth-core`,
`sdlc-shared`, `sdlc-telemetry` сохранены. Source contract выполняется тем же
`ServerCliMetadataTests`; SDK ignore inventory из четырёх крейтов остаётся
полным. Native resolver/guard/layer/old-mtime и отдельный CLI проверяются на
финальной вехе, source assertions не заменяют эту квалификацию.

## Scoped standalone container helper

`python scripts/build.py` использует existing Base plan относительно HEAD.
Явный service задаёт один scope; `--all` выбирает все buildable services.
Пустой clean/docs/test-only выбор не вызывает verifier/Docker и не является
readiness/artifact proof. Cold bootstrap выбирает service явно. Packaging
recipes/nginx имеют отдельную карту; unknown paths сохраняют product fallback.
Invalid ref или unsupported map отказывают до подготовки. SDK/UI pins независимы;
несовместимый single-context mixed build требует qualified multi-pin route.
Raw heavy остаётся под общим host slot, Root owner вторым slot не оборачивать.
Source команда: `python -B -m unittest scripts.tests.test_build_scope -q`.
Контракт/границы: [план](plans/container-selection-20261030.md).

Forge backend producer выбирает backend и runner: у них один recipe, но разные
automatic image tags. `--all` включает также frontend; CLI/delivery/migrations
сохраняют отдельные producers и не выдумывают container build.
