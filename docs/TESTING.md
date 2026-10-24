# Стратегия тестирования Forge CI/CD — Moved

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
