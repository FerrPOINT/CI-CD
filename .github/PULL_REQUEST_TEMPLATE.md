# Pull Request

## Описание

Что и зачем меняет этот PR. Ссылка на issue: `Closes #N` (если применимо).

## Тип изменения

- [ ] `feat` — новая функциональность
- [ ] `fix` — исправление бага
- [ ] `docs` — только документация
- [ ] `refactor` — без изменения поведения
- [ ] `test` / `chore` / `perf` — прочее

## Чек-лист

- [ ] Коммиты следуют Conventional Commits (`feat:`, `fix:`, `docs:`, ...).
- [ ] Тесты добавлены/обновлены и проходят: backend `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, integration DB tests, `cargo build --release --workspace`; frontend `pnpm openapi:check`, `pnpm openapi:compat --base-ref origin/main`, `pnpm lint`, `pnpm test`, `pnpm build`.
- [ ] Применимые CI gates зелёные: docs, compose-smoke, e2e Playwright/axe и security scan (`cargo audit`, `pnpm audit`, secret scan, SBOM drift, Trivy container image scan).
- [ ] `docs/API.md` обновлён при изменении API (endpoint, форматы, коды ответов).
- [ ] `docs/DATA_MODEL.md` обновлён при изменении схемы БД или запросов.
- [ ] Новый ADR в `docs/adr/` для архитектурного решения (или обновлён существующий).
- [ ] Скриншоты UI-проверки приложены к PR или CI artifacts (375 / 1920 / 2560); в Git остаются только поддерживаемые ресурсы документации и fixtures.
- [ ] Новые endpoint проверены curl-ом; `docker compose config -q` проходит.
- [ ] `CHANGELOG.md` дополнен в `[Unreleased]` для пользовательских изменений.
- [ ] В diff нет `.env`, токенов, паролей и других секретов (только `.env.example`).
