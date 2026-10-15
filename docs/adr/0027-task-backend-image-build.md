# ADR-0027: backend-owned image build в task pipeline

## Status

Source candidate. Rust/PG/TLS qualification и live deployment ещё не подтверждены.
Hermes и его upstream-протокол не изменяются.

## Context

Для завершения одной SDLC-задачи DevOps нужен настоящий OCI artifact с exact
commit SHA. Shell job не должен получать Docker socket или backend TLS keys.
Предварительно загруженный образ не доказывает сборку изменений текущей задачи.

## Decision

Repository `.forge-ci.yml` v1 может определить закрытый `image_build` вместо
`commands`. Действие разрешено только authenticated `task_intent` pipeline.
Существующие queue, embedded lease, submission CAS, monitor и artifact collector
остаются единственным путём исполнения; нового scheduler/controller нет.

План фиксирует relative context/Dockerfile/target, descriptor path и совместимые
версии readonly schema. Job не задаёт daemon, image/tag, build args, secrets,
entitlements или host network. Backend экспортирует exact Git objects в
приватный sealed attempt context вне job mounts. Buildx v0.37.2 использует
существующую проверенную Docker/TLS authority backend, default Docker driver и
`--load`. Docker authority не передаётся агентам или job-контейнерам.

Перед spawn сохраняется original attempt custody и проверяется current Tracker
authority. Успех CLI требует actual same-daemon image inspect с exact
source/attempt/request labels. Descriptor `forge/oci-candidate/v1` создаётся
backend, а не читается из изменяемого checkout. Перед artifact publication
повторно проверяются task authority, current attempt, lease deadline и cancel.

Неизвестный исход, timeout и неподтверждённый remote solver удерживают attempt,
не разрешая автоматический rebuild. Disposable context/output удаляются только
после known terminal DB ACK; image и маленькое immutable custody evidence
сохраняются. Retry проверяет и image-build, и прежний Compose journal.

Authenticated HTTP OCI delivery использует тот же настроенный TLS daemon с
точным published policy daemon ID. Legacy local CLI сохраняет Unix transport;
исторический GET не требует живого Docker. Никаких новых mounts/credentials.

## Consequences

Артефакт связан с реальным исходником задачи без Docker credentials внутри job.
Build descriptor не означает deployment, acceptance или завершение стадии:
DevOps обязан получить отдельные delivery, health и acceptance receipts.

Read-only data protocol остаётся `readonly_snapshot_v1`, migrations пусты.
Mutable PostgreSQL delivery не становится частью этого image-build действия.
Unknown build требует reconciliation; exit клиента не доказывает stop BuildKit.
Actual qualification обязательна отдельно от source tests и packaging parity.

## Alternatives

- Docker внутри job: отвергнуто из-за передачи backend authority.
- Отдельный controller или queue: не нужен, уже есть embedded runner.
- Обычная shell-команда `docker build`: не задаёт закрытый typed contract и
  не отделяет mutable job filesystem от backend artifact custody.
- Только preloaded image: недостаточно для сборки результата текущей задачи.
