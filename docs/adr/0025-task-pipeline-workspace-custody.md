# ADR-0025: task intent в существующем pipeline и embedded runner

Статус: source candidate; migration/policy/OpenAPI registration подключены,
генерация спецификации и runtime qualification pending.

## Проблема

Generic pipeline создаёт настоящий checkout, но не связывает его с immutable
task workspace operation. HTTP `workspace prepare` лишь наблюдает уже известные
runner identities. Agent не должен получать runner credential или придумывать
lease/generation/nonce. Filesystem primitive сам по себе не является admission.

## Решение

Optional typed intent хранится в той же транзакции с прежними pipeline/jobs/queue.
Используются existing dedicated SAT/project ACL, namespace/repository mapping
и server-only Tracker reader. Никаких новых issuer, очередей или controllers.

Выбранная embedded preparation job сохраняет actual identities до первого Git
IO и вызывает existing filesystem preparation/readback. Original request не
сбрасывается после ошибки; missing/active/partial journal не разрешает clone.
Перед receipt и command проверяется свежий current tuple. Job submission CAS
сохраняет original attempt до spawn; unknown/restart не повторяет command.
Completion evidence отмечается только по existing embedded terminal readback
и local completion journal, не по таймеру/StopACK.

Authenticated locator читает прежний immutable blocked receipt после cleanup.
Физическая preparation не разрешает writable access, Tracker transition или
deployment. Candidate/DevOps delivery сохраняют собственные existing guards.
Legacy trigger без intent не меняется; external task preparation закрыта, пока
нет owner-authenticated physical receipt adapter.

## Границы

Одна additive migration и OpenAPI registration подключены из source;
released DDL, Hermes, SDK, dependency locks и accepted stand не меняются.
Crash после custody commit до Git/spawn может консервативно удержать работу:
отсутствие effect не выдумывается и attempt/key автоматически не сбрасываются.
Existing runner reconciliation остаётся границей наблюдения неизвестного child.
PG/HTTP/runner qualification и CLI packaging нужны отдельно; source tests не
объявляют полную SDLC acceptance.
