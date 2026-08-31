# Contributing — Forge CI/CD

Спасибо за интерес к проекту! Пул-реквесты, issue и обсуждения приветствуются.
Рабочий язык проекта — русский (код и комментарии — английский, см. `docs/CODE_STYLE.md`).

## Как внести вклад

1. Сделайте fork и создайте ветку от `main`.
2. Опишите проблему или задачу в [Issues](https://github.com/FerrPOINT/CI-CD/issues) перед крупными изменениями.
3. Откройте pull request в `main` по шаблону `.github/PULL_REQUEST_TEMPLATE.md`.

## Настройка окружения

Полное руководство по локальной сборке и запуску — [`docs/DEVELOPMENT_GUIDE.md`](docs/DEVELOPMENT_GUIDE.md):

```bash
git clone git@github.com:FerrPOINT/CI-CD.git
cd CI-CD
cp .env.example .env
docker compose up -d --build
curl -sS http://localhost:22801/api/v1/health
```

Быстрая справка по проверкам — `AGENTS.md` (раздел «Docker и команды проверки») и `justfile` (`just test-backend`, `just test-frontend`).

## Conventional Commits

Коммиты следуют [Conventional Commits](https://www.conventionalcommits.org/ru/v1.0.0/):

```text
feat: добавить фильтр проектов по репозиторию
fix: исправить NUMERIC cast в отчётах
docs: обновить docs/API.md после добавления endpoint
refactor: вынести переходы статусов в cicd-domain
test: покрыть confirm-dialog компонент
chore: обновить зависимости
perf: кэшировать список пайплайнов
```

- Один коммит — одна логическая единица.
- Scope указывайте при неоднозначности: `feat(ui):`, `fix(api):`.
- BREAKING CHANGE описывайте в footer коммита и в `CHANGELOG.md` (`[Unreleased]`).

## Чек-лист pull request

Перед отправкой PR убедитесь:

- [ ] Изменение capability/контракта: обновлены `TRACEABILITY.md` (REQ-ID) и, при новой границе доверия, `THREAT_MODEL.md`.
- [ ] Тесты пройдены: backend `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`; frontend `pnpm test`, `pnpm build`.
- [ ] Документация обновлена: `docs/API.md` при изменении API, `docs/DATA_MODEL.md` при изменении схемы БД, новый ADR в `docs/adr/` для архитектурных решений.
- [ ] Скриншоты приложены для UI-изменений (full-page: 375 / 1920 / 2560, в `docs/screenshots/`).
- [ ] В diff **нет** `.env`, токенов, паролей и других секретов — только `.env.example`.
- [ ] Новые endpoint проверены curl-ом; `docker compose config -q` проходит.
- [ ] `CHANGELOG.md` дополнен в `[Unreleased]`, если изменение пользовательское.

## Стиль кода

- Backend: слои `api → domain → store`; переходы статусов только через `JobStatus::transition_to()`; SQL — параметризованные запросы SQLx.
- Frontend: React + Tailwind + shadcn/ui, типизированные API-клиенты и DTO.
- Подробности — `docs/CODE_STYLE.md`, `docs/CODE_REVIEW.md`, `docs/TESTING.md`.

## Лицензия и права на вклад

Проект является proprietary source-available, not open source. PR принимается только если contributor дает FerrPOINT irrevocable, worldwide, perpetual, royalty-free, sublicensable, transferable right to use, reproduce, modify, distribute, relicense, commercialize, and sell the contribution as part of the software or related products and services.

Если вы не согласны с этим grant of rights, не отправляйте PR, patch, documentation change, design, review suggestion или иной contribution.

## Сообщения об уязвимостях

Уязвимости сообщайте приватно — процесс описан в [`SECURITY.md`](SECURITY.md), не через публичные issue.
