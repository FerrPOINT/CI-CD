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

## CLI metadata в server Docker recipes

Server image сохраняет `cicd-server` и `forge-runner`; отдельный `cicd-cli`
представлен только Cargo.toml и synthetic main с `compile_error!`. Полный CLI
доступен своему producer и ручным release checks. Изменение production graph,
которое добавляет CLI как зависимость, требует пересмотра этой границы.
Из корня репозитория: `python -B -m unittest scripts.tests.test_ci_contract_checks.ServerCliMetadataTests`.
Existing tooling entrypoint выбирает этот source contract. Native locked/layer/
guard/CLI qualification остаётся на общей финальной вехе; source checks её
не заменяют. Full candidate key/export и SDK/UI/tooling pins этим не сужаются.
