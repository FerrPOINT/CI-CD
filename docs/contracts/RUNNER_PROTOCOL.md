# Runner protocol

**Статус:** Current verified MVP + Target approved. Реализованный subset на 2026-09-01 покрывает `register`, `heartbeat`, `work:poll` с optional long-poll `waitSeconds` и wakeup через in-process signal + PostgreSQL `LISTEN/NOTIFY`, basic tag matching, current `shell` executor capability matching, `ack`, lease-scoped `control`/`secrets:resolve`, one-shot artifact upload, `renew`, `logs`, `complete`, cancel signal delivery через `job_leases.cancel_requested_at`, SHA-256 storage для runner credential/lease token, fencing по `job_leases.generation`, `workspace.checkoutUrl`, declared `attempt.secrets`/`attempt.artifacts` и отдельный `forge-runner` shell process. Остальные разделы описывают target-контракт production runner-а. Канонические путь и имена таблиц закреплены [ADR-0009](../adr/0009-canonical-registry.md).

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
  "type":"object","required":["protocolVersion","registrationToken","name"],
  "properties":{
    "protocolVersion":{"const":1},
    "registrationToken":{"type":"string","minLength":1},
    "name":{"type":"string","minLength":1,"maxLength":128},
    "tags":{"type":"array","maxItems":64,"items":{"type":"string","pattern":"^[a-z0-9][a-z0-9._-]{0,62}$"}},
    "capabilities":{
      "type":"object",
      "properties":{
        "executorKinds":{"type":"array","maxItems":16,"items":{"type":"string","pattern":"^[a-z0-9][a-z0-9._-]{0,62}$"}}
      },
      "additionalProperties":true
    }
  },"additionalProperties":false
}
```

Успешный `201` возвращает `{protocolVersion, runnerId, credential, credentialExpiresAt, heartbeatIntervalSeconds, pollWaitMaxSeconds}`. `credential` показывается ровно один раз. Current MVP сверяет `registrationToken` с `CICD_RUNNER_REGISTRATION_TOKEN`, нормализует optional `tags`, принимает optional object `capabilities`, валидирует optional `capabilities.executorKinds` и хранит credential hash + hint; одноразовые/scoped registration tokens, protected tags и registration-token audit остаются target.

`pollWaitMaxSeconds` равен `30`: runner может запросить long-poll wait от `0` до `30` секунд, а `0` сохраняет immediate-poll совместимость.

`POST /api/v1/runner/heartbeat` принимает:

```json
{"protocolVersion":1,"status":"online","draining":false,"capacity":{"totalSlots":4,"busySlots":1},"tags":["linux","docker"],"capabilities":{},"activeLeaseIds":["uuid"]}
```

Схема: `status` равен `online` или `draining`; `totalSlots` - 1..1024, `busySlots` - 0..`totalSlots`; current MVP нормализует optional tags, при наличии заменяет stored tags, при отсутствии сохраняет текущие tags, сохраняет optional object capabilities, валидирует optional `capabilities.executorKinds` и обновляет heartbeat snapshot. Во время active lease runner продолжает слать heartbeat с занятым слотом и `activeLeaseIds`; `forge-runner` делает это каждые 15 секунд после `ack` до terminal completion. Ответ `204`. Protected-tag scope, capability allowlist и запрет изменения protected tags остаются target policy.

## 3. Получение и подтверждение работы

`POST /api/v1/runner/work:poll`:

```json
{"type":"object","required":["protocolVersion","capacity"],"properties":{
  "protocolVersion":{"const":1},
  "capacity":{"type":"object","required":["freeSlots"],"properties":{"freeSlots":{"type":"integer","minimum":0,"maximum":1024}},"additionalProperties":false},
  "waitSeconds":{"type":"integer","minimum":0,"maximum":30},
  "tags":{"type":"array","maxItems":64,"items":{"type":"string","pattern":"^[a-z0-9][a-z0-9._-]{0,62}$"}},
  "capabilityDigest":{"type":"string","maxLength":128}
},"additionalProperties":false}
```

Current MVP сначала делает immediate claim; если совместимой работы нет и `waitSeconds > 0`, сервер ждёт не дольше указанного бюджета и просыпается по in-process signal после committed enqueue/разблокировки следующей стадии либо по PostgreSQL `LISTEN runner_work_available`, который создаётся DB-trigger-ами на queued `job_queue` rows и unblock-события `jobs.status/manual`. `204` означает, что за этот интервал совместимой работы не появилось. Optional `tags` в poll нормализуются; пустой список означает текущие stored runner tags, непустой список должен быть subset stored runner tags и может сузить выдачу. Текущий executor offer равен `shell`: runner получает работу, если `capabilities.executorKinds` отсутствует или явно содержит `shell`; runner с явным списком без `shell` считается несовместимым. `LISTEN/NOTIFY` является только wakeup-ускорителем; durable source of truth остаётся `job_queue` claim через `SKIP LOCKED`, а broker-level fairness остаётся target. При найденной работе ответ `200 LeaseOffer`:

```json
{"protocolVersion":1,"leaseId":"uuid","leaseToken":"opaque","fencingToken":7,
 "ackDeadline":"2026-08-27T12:00:30Z","leaseExpiresAt":"2026-08-27T12:02:00Z",
 "attempt":{"id":"uuid","number":1,"pipelineId":"uuid","jobId":"uuid","jobKey":"test","commitSha":"<full-sha>",
 "executor":"shell","image":"rust@sha256:<digest>","commands":["cargo test"],"environment":{},
 "secrets":["DEPLOY_TOKEN"],"timeoutSeconds":3600,
 "workspace":{"checkout":true,"checkoutUrl":"https://forge.example/git/project.git"},"artifacts":["target/release/app.tar.gz"]},
 "planSignature":{"kid":"string","signature":"base64url"}}
```

Offer содержит только имена declared secrets и relative paths declared artifacts, но не secret values и не storage credentials. Current `forge-runner` использует `workspace.checkoutUrl` для `git clone`, после `ack` получает scoped secret bundle, затем выполняет команды shell в workspace, передаёт declared secrets в env, отправляет stdout/stderr через protocol log append с best-effort masking, загружает declared artifact files и отправляет terminal result; `image` остаётся compatibility field до Docker/Kubernetes runner. Target runner проверяет `planSignature`, не запускает работу до ack и не сохраняет `leaseToken` в log/metadata. Несовместимый/disabled/draining/offline runner, runner без нужных `required_tags` или runner с explicit `executorKinds` без `shell` не получает offer.

`POST /api/v1/runner/leases/{leaseId}/ack` и `POST /api/v1/runner/leases/{leaseId}/renew` используют одну схему:

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string","minLength":1},"fencingToken":{"type":"integer","minimum":1}
},"additionalProperties":false}
```

Ack допускается только до `ackDeadline`; ответ: `{protocolVersion, leaseExpiresAt, renewAfter, cancelRequested}`. Current reconciler после `ackDeadline` закрывает unacknowledged lease как expired delivery и возвращает тот же `job_queue`/job/attempt в `queued` для следующего claim; поздний ack старой lease получает `410`. Renew допускается только для active, acknowledged, неистёкшей lease и возвращает те же поля. Повтор корректного ack/renew идемпотентен; stale token/generation возвращает `409 lease_fenced` (или `410` после окончательного expiry).

`GET /api/v1/runner/leases/{leaseId}/control` возвращает те же control fields без продления lease. В current MVP control fields передаются headers: `Authorization: Bearer <runner-credential>`, `X-Runner-Protocol-Version: 1`, `X-Lease-Token: <opaque>`, `X-Fencing-Token: <generation>`. Endpoint доступен только owner-у acknowledged active lease. User-facing cancel для external lease выставляет `cancel_requested_at`, после чего `control`, `ack` и `renew` возвращают `cancelRequested: true`; `forge-runner` polling-ит этот endpoint во время long-running shell command, завершает процесс и отправляет `complete` с `outcome: "canceled"`. Если user cancel уже перевёл job/attempt в `canceled`, сервер принимает только подтверждённый `outcome: "canceled"` от того же active lease; конкурентный `success` rejected как fenced/conflict.

## 4. Секреты, артефакты, логи и завершение

После успешного ack owner получает declared secrets через `POST /api/v1/runner/leases/{leaseId}/secrets:resolve`. Current сервер принимает только active acknowledged lease, сверяет runner credential, lease token hash, `fencingToken` и возвращает только requested names из `jobs.required_secrets`; запрос чужого или не объявленного secret name возвращает `403`.

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken","secretNames"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string"},"fencingToken":{"type":"integer","minimum":1},
 "secretNames":{"type":"array","minItems":1,"maxItems":64,"items":{"type":"string","pattern":"^[A-Z][A-Z0-9_]{0,127}$"}}
},"additionalProperties":false}
```

Ответ `200` имеет `{protocolVersion, expiresAt, items:[{name, injection:"env", value}]}`. `value` существует только в защищённом TLS-ответе, не записывается в БД/аудит/логи; current `forge-runner` хранит значения только в памяти процесса, передаёт их в env и маскирует stdout/stderr best-effort. Сервер возвращает только ключи immutable plan; ответ `Cache-Control: no-store`. File injection, KMS-backed short leases, rotation policy и full redaction во всех trace/error каналах остаются target hardening.

`POST /api/v1/runner/leases/{leaseId}/artifacts`:

Current MVP принимает один файл в raw request body. Control fields передаются headers, чтобы artifact bytes не кодировались в JSON:

| Header | Значение |
|---|---|
| `Authorization` | `Bearer <runner-credential>` |
| `X-Runner-Protocol-Version` | `1` |
| `X-Lease-Token` | opaque lease token |
| `X-Fencing-Token` | `job_leases.generation` |
| `X-Attempt-Id` | UUID текущей attempt |
| `X-Artifact-Path` | один из paths из `LeaseOffer.attempt.artifacts` |
| `X-Artifact-Name` | display name без `/`, `\`, quotes и control chars |
| `Content-Type` | MIME type файла; default `application/octet-stream` |

Запрос принимается только после `ack`, пока lease active, `job` и `attempt` находятся в `running`, runner identity, lease token hash, `fencingToken` и `attemptId` совпадают, а `X-Artifact-Path` входит в `jobs.artifact_paths` этой job. Body должен быть 1 byte..50 MiB. Сервер сохраняет bytes в `CICD_ARTIFACTS_DIR`, пишет `artifacts` metadata с `attempt_id`, `content_type`, `size_bytes`, `sha256` и возвращает обычный `Artifact` JSON. Current `forge-runner` загружает только file paths из `LeaseOffer.attempt.artifacts`; directory upload, glob patterns, chunked/resumable sessions, per-artifact retention и object storage credentials остаются target hardening.

`POST /api/v1/runner/leases/{leaseId}/logs`:

Current MVP принимает server-sequenced строки и пишет их в существующие `job_logs` с префиксом stream:

```json
{"type":"object","required":["protocolVersion","leaseToken","fencingToken","attemptId","lines"],"properties":{
 "protocolVersion":{"const":1},"leaseToken":{"type":"string"},"fencingToken":{"type":"integer","minimum":1},"attemptId":{"type":"string","format":"uuid"},
 "lines":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object","required":["message"],"properties":{
   "stream":{"enum":["stdout","stderr","system"],"default":"stdout"},"message":{"type":"string","maxLength":8192}
 },"additionalProperties":false}}
},"additionalProperties":false}
```

Ответ `200` имеет `{protocolVersion, accepted, nextAfter}`. `message` является одной строкой лога: trailing CR/LF от stream-reader допускается и обрезается, вложенные переводы строк отклоняются. Запрос принимается только после `ack`, пока lease active, `job` и `attempt` находятся в `running`, а `leaseToken`, `fencingToken` и `attemptId` совпадают. После completion/expiry stale log append возвращает `409` или `410`.

Target chunked upload заменяет server-generated sequence на idempotent runner sequence:

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

Completion terminal и идемпотентен только для идентичных данных той же active lease. Сервер применяет принятый cancel intent с приоритетом над конкурентным completion: после user cancel external runner может закрыть active lease только `outcome: "canceled"`. `409`/`410` требуют от runner немедленно прекратить stale execution и удалить workspace/backend resource.

## 5. Lease, timeout и состояния

| Параметр | Default | Допустимый диапазон | Норма |
|---|---:|---:|---|
| registration token TTL | 1h | 5m..24h | атомарно расходуется при register |
| heartbeat interval | 15s | 5s..60s | current MVP переводит stale online runner без unexpired active lease в `offline` после 120s; `unhealthy` остаётся target state |
| poll wait | 0s default / 30s max | 0s..30s | `0` immediate, `>0` bounded ожидание offer с in-process + PostgreSQL `LISTEN/NOTIFY` wakeup |
| ack timeout | 30s | 10s..120s | current MVP закрывает unacknowledged lease как expired delivery и requeue-ит attempt; target может выделить `abandoned` state |
| lease TTL | 120s | 30s..600s | продлевается только owner-ом |
| renew interval | 40s | `< TTL / 2` | runner начинает renew до deadline |
| execution timeout | 1h | 1m..24h | фиксируется в attempt snapshot |
| cancel grace | 30s | 1s..10m | затем forced termination |

Все значения копируются в `execution_attempts`/`job_leases` при создании и не меняются для уже выданной работы. Lease expiry fencing-ит прежнего owner до retry; повторная доставка создаёт новую attempt, а не переиспользует terminal attempt.

| Сущность | Состояния и допустимый переход |
|---|---|
| Runner | `registered -> online -> draining|unhealthy|offline|disabled|revoked`; только `online` получает work |
| Queue | Current MVP: `queued -> leased -> queued` при ack timeout или `leased -> completed|canceled` при terminal/cancel; long-poll ждёт только до claim и не вводит отдельное состояние; target может split-ить pre-ack `offered/abandoned`; claim выполняется транзакционно через `SKIP LOCKED` |
| Lease | Current MVP: `active -> completed|expired|canceled`; unacknowledged lease после `ackDeadline` становится `expired`, а queue row получает новый claim с новой generation; target может split-ить `offered -> active`, `offered -> abandoned` при ack timeout |
| Attempt | `queued -> leasing -> assigned -> running -> success|failed|canceled|timed_out|lost`; retry создаёт следующий номер |

Мутирующая операция проверяет одновременно authenticated `runnerId`, `leaseId`, HMAC token, `fencingToken`, owner, active state и `expiresAt > now()`. Для одного attempt существует не более одной active lease; `job_leases` хранит историю, а не один mutable lease.

## 6. Совместимость

- Сервер и runner обязаны посылать/проверять `protocolVersion`; отсутствие или неподдерживаемое значение возвращает `422 validation_failed` с detail `unsupported_protocol_version`.
- Версия `1` допускает только аддитивные optional response fields. Неизвестные поля ответа runner игнорирует; сервер отклоняет неизвестные request fields, чтобы не принять опечатку как policy.
- Изменение обязательности, типа, семантики, endpoint либо state transition требует следующей major protocolVersion и параллельной поддержки предыдущей версии на срок, объявленный release policy.
- Retry повторяет byte-equivalent mutation с тем же token, fencing token, sequence и outcome; новый poll никогда не переиспользует старую lease.
- Реализация обязана иметь contract tests для текущей и предыдущей поддерживаемой protocolVersion до включения feature flag.
