# DOCUMENTATION GOVERNANCE — Forge CI/CD

Как устроена и изменяется документация. Исполняется `scripts/verify_docs.py`.

## Authority matrix (ADR-0009)

| Порядок | Источник | Что определяет |
|---|---|---|
| 1 | код + `backend/migrations/` + `openapi/openapi.yaml` | фактическое runtime-поведение |
| 2 | `docs/adr/NNNN-*.md` | принятые архитектурные решения; номера не переиспользуются |
| 3 | `docs/contracts/*.md` | нормативные целевые контракты (наблюдаемые требования) |
| 4 | narrative-доки (`AUTHORIZATION.md`, `RUNNER_ARCHITECTURE.md`, …) | объяснения; не вводят канонических имён |
| 5 | `docs/CURRENT_STATE.md` | производный снимок текущего состояния |
| 6 | `plans/*.md` | закоммиченные рабочие планы; не нормативны |

## Статусная таксономия

Каждое capability-утверждение в docs должно иметь статус:

- **Current verified** — работает сейчас, проверено кодом/evidence.
- **Configuration only** — CRUD/формы есть, execution/delivery нет.
- **Target approved** — принято контрактом/ADR, не реализовано.
- **Deprecated/historical** — устарело; содержит redirect.

## Канонический словарь (выдержка ADR-0009)

`backend/migrations/`, `pipeline_plans`, `domain_events`, `outbox_messages`, `outbox_delivery_attempts` (current history), `outbox_deliveries` (target snapshots/leases), `execution_attempts`, `job_queue`, `job_leases`, `/api/v1/runner/*`, `openapi/openapi.yaml`, `tenant` (не organization/workspace), error codes в `snake_case`, `request_id` внутри `error`.

Запрещены как канонические: `outbox_events`, `pipeline_runs`, `job_runs`, `/api/v1/runner/v1/*`, `openapi/openapi.json`.

## Изменение документации

1. Контракт меняется → сначала правится ADR или `docs/contracts/*`, затем narrative, затем производные (CURRENT_STATE, гайды).
2. Новые capability-утверждения обязаны иметь статус из таксономии.
3. Коммит проверяется `python3 scripts/verify_docs.py --all` (ссылки, канон, статусы, сироты).
4. Документы не дублируют normative-контент: ссылаются на owner-документ.
5. Удаление документа — через redirect-stub один release-цикл + запись в CHANGELOG.

## Документная карта

```text
docs/
├── README.md              # карта + статусная легенда
├── CURRENT_STATE.md       # производный снимок current capabilities
├── PRODUCT_REQUIREMENTS.md # REQ/NFR-ID и baseline scope
├── TRACEABILITY.md        # RTM, проверки и evidence
├── TEST_PLAN.md           # стратегия и gate-матрица
├── ARCHITECTURE_INDEX.md  # вход в архитектурные документы
├── ARCHITECTURE.md        # runtime/workspace layout
├── FUNCTIONAL_ARCHITECTURE.md
├── DOMAIN_MODEL.md
├── DATA_MODEL.md
├── USER_GUIDE.md
├── DEVELOPMENT_GUIDE.md
├── OPERATIONS.md
├── ENV.md
├── TROUBLESHOOTING.md
├── contracts/             # нормативные target/current контракты
├── architecture/          # boundaries, sequences, transition map
├── adr/                   # accepted ADRs
├── assets/screens/        # manifest визуальных evidence
└── screenshots/           # PNG evidence для Dashboard routes/states
```

Полная локальная карта документов поддерживается в `docs/README.md`; этот блок
фиксирует только верхнеуровневые обязательные входы и каталоги.
