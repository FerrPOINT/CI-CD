# Поддержка — Forge CI/CD

Сначала загляните в документацию, затем — в существующие обсуждения; если ответа нет, создайте issue.

## Где искать помощь

| Канал | Когда использовать |
|---|---|
| [Issues](https://github.com/FerrPOINT/CI-CD/issues) | Баги и запросы функциональности (шаблоны — `.github/ISSUE_TEMPLATE/`) |
| [Discussions](https://github.com/FerrPOINT/CI-CD/discussions) | Вопросы «как сделать», идеи, использование, общение с сообществом |
| [`SECURITY.md`](SECURITY.md) | Уязвимости — только приватно через GitHub Security Advisories, не в issue |

## Документация

- `README.md` — обзор проекта и скриншоты.
- `docs/LOCAL_SETUP.md` — локальный запуск и переменные окружения.
- `docs/TROUBLESHOOTING.md` — диагностика частых проблем.
- `docs/CURRENT_STATE.md` — что реализовано, а что нет (MVP).
- `docs/API.md`, `docs/DATA_MODEL.md`, `docs/ARCHITECTURE.md` — контракты и внутреннее устройство.

## Перед тем как написать issue

1. Проверьте, не описана ли проблема уже в [issues](https://github.com/FerrPOINT/CI-CD/issues?q=is%3Aissue) и [discussions](https://github.com/FerrPOINT/CI-CD/discussions).
2. Уточните свою версию (коммит или тег) в `docs/CURRENT_STATE.md` — статус «NOT production-safe» может быть причиной поведения.
3. Для бага заполните шаблон «Bug report» (шаги воспроизведения, ожидание/реальность, логи), для идеи — «Feature request».

Внимание: проект в стадии MVP без auth — если проблема связана с безопасностью, пишите приватно через [`SECURITY.md`](SECURITY.md), а не в публичный трекер.
