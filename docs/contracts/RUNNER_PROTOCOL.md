# Runner protocol

**Статус:** Current verified MVP + Target approved. Реализованный subset на 2026-09-01 покрывает `register`, `heartbeat`, immediate `work:poll`, `ack`, `renew`, `complete`, SHA-256 storage для runner credential/lease token, fencing по `job_leases.generation`, `workspace.checkoutUrl` и отдельный `forge-runner` shell process. Остальные разделы описывают target-контракт production runner-а. Канонические путь и имена таблиц закреплены [ADR-0009](../adr/0009-canonical-registry.md).

## 1. Область и общие правила

Протокол обслуживается на `/api/v1/runner/*`; production deployment обязан ставить TLS/reverse proxy перед API. Каждый запрос после регистрации использует `Authorization: Bearer <runner-credential>`; registration token применяется только к `register`. Все JSON-тела используют UTF-8, lower camel case и обязательное поле `protocolVersion`.

`protocolVersion` в этом документе равен `1`. Идентификаторы являются UUID, время - RFC 3339 UTC, а `commitSha` - полный 40- или 64-символьный hex SHA. Ответы не содержат DB credentials, control-plane token, Docker socket либо plaintext секреты вне `secrets:resolve`.

Ошибки используют envelope из `docs/IMPLEMENTATION_CONTRACTS.md`: `401` invalid/revoked credential, `403` scope запрещён, `409` `lease_fenced`/sequence conflict, `410` expired lease, `422` validation failure, `429` rate limit, `503` temporary dependency failure. Runner повторяет только transport/`429`/`503` с exponential full-jitter backoff.

Общие фрагменты JSON Schema (Draft 2020-12):

```json
{
  "$defs": {
    "protocol": {"type":"integer","const":1},
    "uuid": {"type":"string","format":"uuid"},
    "time": {"type":"string","format":"date-time"},
    "fencing": {"type":"integer","minimum":1},
    "sha256": {"type":"string","pattern":"^[A-Fa-f0-9]{64}$"}
  }
}
```

Все mutation-запросы, кроме `register`, содержат `protocolVersion`. `leaseToken` - opaque bearer secret; current MVP хранит SHA-256 hash, target hardening переходит на HMAC-SHA-256/pepper. `fencingToken` - монотонное значение `job_leases.generation`; оно обязательно в каждом запросе, изменяющем lease, attempt, logs, artifacts или secrets.

## 2. Регистрация и heartbeat

`POST /api/v1/runner/register`:

```json
{
  "type":"object","required":["protocolVersion","registrationToken","name","tags","capabilities"],
  "properties":{
    "protocolVersion":{"const":1},
    "registrationToken":{"type":"string","minLength":1},
    "name":{"type":"string","minLength":1,"maxLength":128},
    "tags":{"type":"array","maxItems":64,"items":{"type":"string","pattern":"^[a-z0-9][a-z0-9._-]{0,62}$"}},
    "capabilities":{"type":"object","required":["executorKinds","os","arch"],"properties":{
      "executorKinds":{"type":"array","minItems":1,"items":{"enum":["shell","docker","kubernetes"]}},
      "os":{"const":"linux"},"arch":{"enum":["amd64","arm64"]},
      "maxCpuMillis":{"type":"integer","minimum":1},"maxMemoryMiB":{"type":"integer","minimum":1}
    },"additionalProperties":true}
  },"additionalProperties":false
}
```

Успешный `201` возвращает `{protocolVersion, runnerId, credential, credentialExpiresAt, heartbeatIntervalSeconds, pollWaitMaxSeconds}`. `credential` показывается ровно один раз. Current MVP сверяет `registrationToken` с `CICD_RUNNER_REGISTRATION_TOKEN` и хранит credential hash + hint; одноразовые/scoped registration tokens, protected tags и registration-token audit остаются target.

`POST /api/v1/runner/heartbeat` принимает:

```json
{"protocolVersion":1,"status":"online","draining":false,"capacity":{"totalSlots":4,"busySlots":1},"tags":["linux","docker"],"capabilities":{},"activeLeaseIds":["uuid"]}
```

Схема: `status` равен `online` или `draining`; `totalSlots` - 1..1024, `busySlots` - 0..`totalSlots`; tags/capabilities должны соответствовать зарегистрированному scope. Ответ `204`. Heartbeat не продлевает lease и не разрешает изменение protected tags.

## 3. Получение и подтверждение работы

`POST /api/v1/runner/work:poll`:

```json
{"type":"object","required":["protocolVersion","capacity","tags"],"properties":{
  "protocolVersion":{"const":1},
  "capacity":{"type":"object","required":["freeSlots"],"properties":{"freeSlots":{"type":"integer","minimum":0,"maximum":1024}},"additionalProperties":false},
  "tags":{"type":"array","maxItems":64,"items":{"type":"string"}},
  "capabilityDigest":{"type":"string","maxLength":128}
},"additionalProperties":false}
```

Current MVP отвечает сразу и возвращает `204`, если совместимой работы нет; target сервер long-poll-ит не более `pollWait`. При найденной работе ответ `200 LeaseOffer`:

```json
{"protocolVersion":1,"leaseId":"uuid","leaseToken":"opaque","fencingToken":7,
 "ackDeadline":"2026-08-27T12:00:30Z","leaseExpiresAt":"2026-08-27T12:02:00Z",
 "attempt":{"id":"uuid","number":1,"pipelineId":"uuid","jobKey":"test","commitSha":"<full-sha>",
 "executor":"shell","image":"rust@sha256:<digest>","commands":["cargo test"],"environment":{},
 "timeoutSeconds":3600,"workspace":{"checkout":true,"checkoutUrl":"https://forge.example/git/project.git"},"artifacts":[]},
 "planSignature":{"kid":"string","signature":"base64url"}}
```

Offer не содержит секреты. Current `forge-runner` использует `workspace.checkoutUrl` для `git clone`, затем выполняет команды shell в workspace и отправляет terminal result; `image` остаётся compatibility field до Docker/Kubernetes runner. Target runner проверяет `planSignature`, не запускает работу до ack и не сохраняет `leaseToken` в log/metadata. Несовместимый/disabled/draining/offline runner не получает offer.

`POST /api/v1/runner/leases/{leaseId}/ack` и `POST /api/v1/runner/leases/{leaseId}/renew` используют одну схему:

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string","minLength":1},"fencingToken":{"type":"integer","minimum":1}
},"additionalProperties":false}
```

Ack допускается только до `ackDeadline`; ответ: `{protocolVersion, leaseExpiresAt, renewAfter, cancelRequested}`. Renew допускается только для active, неистёкшей lease и возвращает те же поля. Повтор корректного ack/renew идемпотентен; stale token/generation возвращает `409 lease_fenced` (или `410` после окончательного expiry).

## 4. Секреты, логи и завершение

После успешного ack owner получает declared secrets через `POST /api/v1/runner/leases/{leaseId}/secrets:resolve`:

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken","secretNames"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string"},"fencingToken":{"type":"integer","minimum":1},
 "secretNames":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"string","pattern":"^[A-Z][A-Z0-9_]{0,127}$"}}
},"additionalProperties":false}
```

Ответ `200` имеет `{protocolVersion, expiresAt, items:[{name, injection:"env"|"file", value}]}`. `value` существует только в защищённом TLS-ответе, не записывается в БД/аудит/логи и очищается runner-ом из памяти или temporary `0600` file после attempt. Сервер возвращает только ключи immutable plan; ответ `Cache-Control: no-store`.

`POST /api/v1/runner/leases/{leaseId}/logs`:

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken","attemptId","chunks"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string"},"fencingToken":{"type":"integer","minimum":1},"attemptId":{"type":"string","format":"uuid"},
 "chunks":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","required":["sequence","stream","sha256","payload"],"properties":{
   "sequence":{"type":"integer","minimum":1},"stream":{"enum":["stdout","stderr","system"]},"sha256":{"type":"string","pattern":"^[A-Fa-f0-9]{64}$"},"payload":{"type":"string","maxLength":65536}
 },"additionalProperties":false}}
},"additionalProperties":false}
```

Максимальный request body - 1 MiB. Payload уже redacted; сервер применяет второй redaction layer. `(attemptId, sequence, sha256)` повторяется идемпотентно (`202`); такой же sequence с иным hash - `409`. Последовательность задаёт runner, сервер не вычисляет `MAX(sequence)+1`.

`POST /api/v1/runner/leases/{leaseId}/complete`:

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken","attemptId","outcome","finishedAt"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string"},"fencingToken":{"type":"integer","minimum":1},"attemptId":{"type":"string","format":"uuid"},
 "outcome":{"enum":["success","failed","canceled","timed_out","lost"]},"exitCode":{"type":["integer","null"],"minimum":0,"maximum":255},
 "finishedAt":{"type":"string","format":"date-time"},"diagnostic":{"type":"string","maxLength":4096}
},"additionalProperties":false}
```

Completion terminal и идемпотентен только для идентичных данных той же active lease. Сервер применяет принятый cancel intent с приоритетом над конкурентным completion. `409`/`410` требуют от runner немедленно прекратить stale execution и удалить workspace/backend resource.

## 5. Lease, timeout и состояния

| Параметр | Default | Допустимый диапазон | Норма |
|---|---:|---:|---|
| registration token TTL | 1h | 5m..24h | атомарно расходуется при register |
| heartbeat interval | 15s | 5s..60s | unhealthy после 45s, offline после 120s |
| poll wait | 0s current / 25s target | 0s..30s | current immediate poll; target ожидание offer |
| ack timeout | 30s | 10s..120s | offer без ack становится abandoned |
| lease TTL | 120s | 30s..600s | продлевается только owner-ом |
| renew interval | 40s | `< TTL / 2` | runner начинает renew до deadline |
| execution timeout | 1h | 1m..24h | фиксируется в attempt snapshot |
| cancel grace | 30s | 1s..10m | затем forced termination |

Все значения копируются в `execution_attempts`/`job_leases` при создании и не меняются для уже выданной работы. Lease expiry fencing-ит прежнего owner до retry; повторная доставка создаёт новую attempt, а не переиспользует terminal attempt.

| Сущность | Состояния и допустимый переход |
|---|---|
| Runner | `registered -> online -> draining|unhealthy|offline|disabled|revoked`; только `online` получает work |
| Queue | `queued -> offered -> queued|completed|canceled`; claim выполняется транзакционно через `SKIP LOCKED` |
| Lease | `offered -> active -> completed|expired`; `offered -> abandoned` при ack timeout |
| Attempt | `queued -> leasing -> assigned -> running -> success|failed|canceled|timed_out|lost`; retry создаёт следующий номер |

Мутирующая операция проверяет одновременно authenticated `runnerId`, `leaseId`, HMAC token, `fencingToken`, owner, active state и `expiresAt > now()`. Для одного attempt существует не более одной active lease; `job_leases` хранит историю, а не один mutable lease.

## 6. Совместимость

- Сервер и runner обязаны посылать/проверять `protocolVersion`; отсутствие или неподдерживаемое значение возвращает `422 validation_failed` с detail `unsupported_protocol_version`.
- Версия `1` допускает только аддитивные optional response fields. Неизвестные поля ответа runner игнорирует; сервер отклоняет неизвестные request fields, чтобы не принять опечатку как policy.
- Изменение обязательности, типа, семантики, endpoint либо state transition требует следующей major protocolVersion и параллельной поддержки предыдущей версии на срок, объявленный release policy.
- Retry повторяет byte-equivalent mutation с тем же token, fencing token, sequence и outcome; новый poll никогда не переиспользует старую lease.
- Реализация обязана иметь contract tests для текущей и предыдущей поддерживаемой protocolVersion до включения feature flag.
