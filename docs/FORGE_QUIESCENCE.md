# Приостановка Forge перед согласованным backup

Статус: source-контракт; native qualification и установка — отдельные gates.
Старый production CID на c4e86c1 этим механизмом не управляется.
SDK/UI pins, публичный HTTP/OpenAPI и схема БД сохранены.

## Участники

Для каждого Forge writer process одной установки:

- CICD_QUIESCENCE_DIR — один физический private каталог общей файловой системы
  с действующими Linux flock; каталог 0700, файлы 0600, effective UID контроллера.
  Release owner размещает его на existing runner workspace PVC как private
  /workspaces/.quiescence-control вне всех job subPaths, без нового claim.
  Job не получает ни root mount PVC, ни этот каталог. Env и participant IDs
  принадлежат private release config; source/SDK pins от этого не меняются.
- CICD_QUIESCENCE_PARTICIPANT — уникальное стабильное имя процесса:
  A–Z, a–z, 0–9, underscore/hyphen, до 80 символов.

Без opt-in прежний режим сохраняется. Настроенный процесс без регистрации
не получает effect admission. Restart не снимает persistent request.json.
Повреждённая запись, неизвестная custody или недоступный root дают HOLD.

Регистрация: schemaVersion=1, participant, bootId, kind, processId, processStart,
custodyRoots=[{kind,path}]. Запись сама по себе не доказывает liveness:
coordinator проверяет живой exclusive kernel lock participant. Другой boot,
мёртвая регистрация, новый неожиданный writer или duplicate name запрещают ACK.

Внешний owner составляет полный expected manifest после проверки точных
candidate image/CID, PID/start identity, roles и mount/PVC identities.
Не строить его автоматически из найденных процессов. Отсутствие старого
незарегистрированного writer здесь доказать нельзя. Все roots должны быть
видны coordinator через те же физические mounts; невидимые roots нельзя
пропускать. Не смешивать installations в одном каталоге.

## Владение effects

Новые HTTP requests блокируются до auth/cache DB effects. Health/readiness/
metrics остаются доступны. Существующие lease ACK, renew, control, receipt,
secrets, logs, artifact sessions, complete и task credential renewal разрешены.
Authenticated Git fetch также продолжает работу: только GET info/refs с точным
service=git-upload-pack или POST git-upload-pack с правильным Content-Type,
без дополнительных query aliases. Обычный Git auth по-прежнему обязателен.
Receive-pack закрыт; fetch сохраняет native IO custody и после held ACK закрыт.

Effect lease держит фактический owner до physical completion:

- каждый HTTP handler выполняется retained owner task; disconnect отменяет
  только waiter, включая handlers с tokio filesystem IO;
- detached Git/PR merge сохраняют lease после disconnect;
- post-receive при drain требует живой callback lease того же canonical storage;
- embedded job: claim → spawn → terminal ACK → Compose/workspace cleanup;
- external runner: offer/ACK → spawn → completion/workspace cleanup;
- recovery/maintenance продолжает прежнюю custody, новые commands не запускает;
- outbox/scheduler, sync scheduling/events, catalog, retention и startup hook
  rewrite имеют самостоятельные effect leases;
- blocking journal/deployment/workspace writers сохраняют lease в actual
  blocking worker после отмены ожидающего future;
- privileged delivery CLI и migration CLI используют тот же product-local
  kernel module. Добавлены только dependency edges уже закреплённых libraries.

Readonly SSE/LISTEN не являются writers. Внешние applications и произвольные
owner-controlled deployment targets не становятся участниками Forge; их
data-plane backup barriers принадлежат их owners.

## Private local control

    forge-quiesce --directory /workspaces/.quiescence-control request --operation "$OPERATION"
    forge-quiesce --directory /workspaces/.quiescence-control hold \
      --operation "$OPERATION" --expected /private/expected-forge-participants.json

request закрывает новые effects и выдаёт backupAcknowledged=false.
hold остаётся foreground coordinator. После завершения shared effect leases
он получает exclusive kernel fence и проверяет:

1. Точные живые participants до и после readback.
2. Нет running jobs/attempts, любых active leases (включая expired), leased
   queue rows, external leases без terminal ACK, active/stopping/reconciling
   sync runs, unknown refs, open artifact sessions.
3. Реально завершились PostgreSQL statements/transactions отправленные ранее.
   Неизвестная видимость activity — HOLD.
4. Workspaces ACKed, Kubernetes custody закрыта, Compose/build stop+cleanup
   proofs полны, native Git mutation locks свободны, local delivery имеет
   terminal receipt той же операции и освобождённый target lock.

Queued/manual jobs, future schedules и pending notifications можно сохранить:
новые producers закрыты. Историческую неопределённую custody нельзя удалять
или переписывать ради ACK; требуется original-owner reconciliation.

При отрицательном readback exclusive fence освобождается немедленно, чтобы
старые ACK/cleanup продолжились. backupAcknowledged=true валиден только пока
этот точный foreground coordinator жив и держит kernel fence. Owner наблюдает
его identity/liveness и unchanged participant manifest в течение backup.
Coordinator перепроверяет participants каждые 100 ms. Завершившаяся команда
не является удерживаемым barrier.

При необходимости offline backup после полного held ACK отправить в stdin
этого же живого coordinator:

    exit <тот же operation UUID>

Private exit intent связан с current coordinator nonce, свежим acknowledgement
UUID и точными participant boot identities. Server, external runner, delivery
и migration имеют normal-return exit path: только после пустого local owner
set под exclusive fence. Нет сигналов, job killing или timeout fallback.
Неподдерживаемый kind — HOLD. Coordinator удерживает fence и проверяет
исчезновение именно pinned kernel participant owners, неизменность receipts
и отсутствие replacement writers; только затем печатает
forge/quiescence-exited/v1. Это не Docker/CID readback: внешний owner дополнительно
проверяет pinned process/CID disappearance и отключённую restart replacement
policy. Новый boot, потеря coordinator или stale ACK запрещают приёмку.

После backup, пока coordinator всё ещё жив, для завершения отправить:

    resume <тот же operation UUID>

EOF/crash снимает kernel fence, но admission остаётся закрытым. Прежний ACK
невалиден; получить новый hold с тем же operation. После прерванного coordinator
explicit recovery:

    forge-quiesce --directory /workspaces/.quiescence-control resume --operation "$OPERATION"

Другой coordinator не получает release authority, пока первый держит
coordinator.lock; другой operation UUID marker не принимает и не снимает.
Никакого публичного control API или нового machine credential не добавлено.
Перед effect сохраняется private durable record. Нормальный owner удаляет
его только при завершении; panic/process death оставляет record. Даже при
новом process boot оставшаяся запись запрещает ACK и требует original-owner
reconciliation. Не удалять её вручную ради прохождения backup.

## Legacy bootstrap: отдельный exact-old-image fixture

Новый source не доказывает quiescence старого c4e86c1 CID. Не перезапускать его
ради подключения drain. До изменений сохранить первое исходное DB/storage
состояние и provenance; preliminary online copy не объявлять coherent backup.

Для известного idle legacy случая owner квалифицирует fixture точного старого
image: source-owned per-writer barriers, отсутствие реальных jobs/descendants
и unknown custody, network/origin fence, штатный idle shutdown, post-stop
integrity. Нули DB counts диагностические, не admission fence.

Exact old-image fixture уже дал TERM FAIL:
qa/forge-legacy-idle-20261022/result.safe.json. Direct cicd-server как PID 1
без init/handler не завершился от одного SIGTERM за наблюдаемые 5 s, без OOM.
qa/forge-legacy-idle-20261022/crash.safe.json относится только к контролируемому
SIGKILL пустого изолированного QA; он не квалифицирует TERM и не разрешает
production stop. Описанный ниже legacy protocol остаётся неподтверждённым
bootstrap gate. Новый private exit нельзя применить к старому CID.

К 11 исходным диагностическим counts добавить оба запроса, включая будущие
retry времена:

    SELECT count(*) AS pending_or_claimed_notifications
    FROM outbox_messages
    WHERE delivered_at IS NULL AND failed_at IS NULL;

    SELECT count(*) AS undelivered_pull_events
    FROM repository_sync_refs
    WHERE direction='pull' AND state='applied' AND NOT event_delivered;

Source-owned sequence для fixture:

1. Закрыть внешний origin и новые подключения, не обрывая принятые executions.
2. Получить все repository MutationGuard flock без остановки их владельцев.
   Занятый lock означает native writer. Guard наследуется native Git descendants.
3. Отдельной транзакцией получить row locks всех schedules,
   repository_sync_connections и всех unpurged artifacts:
   scheduler UPDATE precedes fire INSERT; sync scheduler FOR UPDATE OF c
   SKIP LOCKED precedes run INSERT; retention FOR UPDATE SKIP LOCKED precedes
   file unlink. Это конкретные owner contracts, не global DB write-lock.
4. После barriers перечитать counts и проверить already-selected/committed
   claims, notification delivery, sync events и in-flight requests. При
   active/unknown освободить locks и дать owner завершить работу.
5. Только на квалифицированном idle fixture проверить штатный idle shutdown,
   подтвердить завершение процесса, descendants и IO workers; затем проверить
   DB, Git refs/hooks и artifact DB→file hashes перед backup.
   Fallback SIGKILL — не успешный idle shutdown.

В c4e86c1 main.rs не await startup hook rewrite: ensure_post_receive_hooks
запускается через tokio::spawn перед listener; readiness может стать зелёным
раньше. One-shot hook write не использует MutationGuard. Его завершение нужно
доказать отдельно; uptime/readiness сами по себе не доказательство. При
неподтверждённом native IO — HOLD. Fixture обязан покрыть этот race.

Failure cases: acquired/pending lease, lost ACK, unknown native process/container,
неполный participant inventory, pending/claimed outbox или sync event, crossing
cron/retry due time, already-selected retention row, unfinished hook rewrite,
source/CID/mount drift, disconnect, сигнал с pending IO, post-stop hash mismatch.
Не менять baseline и не объявлять первый coherent backup qualified при таком
исходе. Не использовать expiry-as-stop, SIGSTOP, kill активных jobs, hotpatch
или общий DB write-lock вместо source-owned closure.

## Generation handoff и restore

Архив quiescence directory содержит OLD-source participants/boot IDs,
request/held/exit markers, lock files и возможную unknown custody. Lock file
из архива не является живым kernel lock. Их нельзя автоматически удалять,
игнорировать или считать destination enrollment. Старый exited receipt сам
по себе не разрешает новые writers в другой installation.

До запуска первого destination writer внешний release owner должен выполнить
явный generation handoff: связать сохранённый sourceExited receipt с точными
OLD process/CID identities, current operation UUID и destination namespace/PVC
UIDs; проверить retirement OLD generation и записать полный approved mapping
на новые participant identities/roots. Unknown custody остаётся HOLD и
сохраняется для original-owner reconciliation. Автоматического import/retire
в этом source-срезе нет: конкретная native процедура и её restore integrity
qualification — открытый operator gate. Не снимать request и не запускать
destination с другим пустым directory ради обхода старой custody.

## Проверки

    cargo check --locked -p cicd-server -p cicd-migrate --all-targets --features integration
    cargo test --locked -p cicd-server --features integration --lib quiescence -- --test-threads=1

Тесты покрывают actual kernel participant child process, cancellation waiter
при живом blocking writer, private normal child exit и wrong-op/stale-holder/
replacement rejection, strict Git fetch boundary с native Git IO только до
held fence, HTTP admission/continuation и реальный PostgreSQL
unfinished SQL/running job. Native enrollment, Compose/Kubernetes executor и
backup/restore integrity остаются owner gates.
