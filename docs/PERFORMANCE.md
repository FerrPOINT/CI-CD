# Производительность и оптимизация — Forge CI/CD — Moved

> **Redirect.** Этот документ объединён в [OPERATIONS.md ](OPERATIONS.md) и больше не поддерживается здесь.
> Историческая версия доступна в git history. Полная карта консолидации — `docs/DOCUMENTATION_GOVERNANCE.md`; удаление stub-ов запланировано в следующем release-цикле (см. `CHANGELOG.md`).


## Кэш обычной генерации OpenAPI

Обычный postinstall использует exact helper Base из общего V2 tooling packet.
Dependencies устанавливаются без scripts; generation имеет отдельные schema/command/
environment inputs и проверенные outputs. Test-only не запускает генератор.
Ручная команда генерации сохранена для более широких сценариев; внешние refs,
cwd Redocly config и внешний SPEC требуют отдельной квалификации.
Общий контракт: [Base closed-generation](https://github.com/FerrPOINT/services-base/blob/00aa287377e4b390e336af78e63e63a5f62b0f37/docs/plans/frontend-closed-generation-20261026.md);
unit/static выполняются в разработке, integration/slow/release — вручную.

Из корня workspace: `./check-workspace.ps1 -Profile Fast -Repository CI-CD -Component frontend`.
