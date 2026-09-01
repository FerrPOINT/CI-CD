# Требования к продукту Forge CI/CD

## 1. Назначение

Forge CI/CD -- self-hosted control plane для доставки исходного кода: от Git-репозитория и определения pipeline до выполнения задач, сохранения доказательств выполнения, развёртывания и автоматизации интеграций. Продукт предназначен для команд, которым нужен контролируемый локальный или собственный CI/CD-контур без передачи исходного кода и данных исполнения внешнему SaaS-провайдеру.

Forge связывает жизненный цикл `Git push -> pipeline -> execution -> logs/artifacts -> deployment` и предоставляет Dashboard, CLI и программный API как равноправные способы работы с ним.

Это документ требований продукта. Наблюдаемые API-, событийные, авторизационные, runner- и data-lifecycle контракты принадлежат `docs/contracts/`; архитектурные решения -- ADR; фактическое состояние -- `docs/CURRENT_STATE.md`.

## 2. Цели продукта

1. Дать команде единое self-hosted место для запуска и наблюдения CI/CD-пайплайнов, связанных с Git-референсами.
2. Сделать результат каждого запуска объяснимым: пользователь видит статус, попытку выполнения, логи, артефакты и историю развёртывания.
3. Предоставить безопасную основу для разделения доступа к проектам, секретам и операциям доставки.
4. Обеспечить автоматизацию доставки через Git-события, расписания, webhooks и уведомления без потери зафиксированных событий.
5. Сохранить возможность постепенного перехода от встроенного локального исполнения к изолированным внешним runner-ам.

## 3. Персоны и ключевые сценарии

### Разработчик

Разработчик создаёт или подключает проект к репозиторию, отправляет изменения либо запускает pipeline для выбранного референса, проверяет ход выполнения и диагностирует ошибку по логам. При необходимости он получает артефакт, повторяет неуспешное выполнение или отменяет ещё неактуальное.

### Владелец проекта / maintainer

Владелец проекта определяет pipeline в репозитории, управляет доступом команды и настройками проекта, хранит секреты, создаёт окружения, смотрит историю развёртываний и настраивает автоматические триггеры и уведомления. Он отвечает за то, чтобы автоматизация была включена только после явной проверки её доставки.

### Оператор CI/CD

Оператор разворачивает Forge в доверенном контуре, поддерживает хранилища Git, PostgreSQL и артефактов, наблюдает здоровье системы, runner-ы и аудит. Он проверяет резервное копирование и восстановление, а также расследует сбои исполнения или доставки интеграций.

### Администратор безопасности

Администратор управляет пользователями, ролями и токенами, задаёт политику доступа и изучает audit trail. Его ключевой сценарий -- убедиться, что секреты не раскрываются, доступ к проекту проверяется до выполнения чувствительной операции, а действия можно проследить.

### Системы-интеграции

Внешний Git-провайдер, webhook-получатель или система уведомлений инициирует либо получает событие. Для неё Forge должен обеспечивать идемпотентную и наблюдаемую доставку с безопасной аутентификацией и повторными попытками там, где это определено контрактом.

## 4. Границы продукта

Forge отвечает за Git-источник в минимально необходимом объёме, определение и выполнение CI/CD, доказательства выполнения, delivery metadata, автоматизацию и управление доступом.

Forge **не является** и не должен развиваться как:

- issue tracker: задачи, бэклоги, спринты, доски и workflow тикетов не входят в продукт;
- package/container registry: публикация, проксирование и жизненный цикл образов или пакетов не входят в продукт;
- IDE или web editor: редактирование исходного кода, интерактивная разработка и выполнение developer workspace в браузере не входят в продукт;
- полноценная замена GitHub, GitLab или Gitea: Forge не обязан предоставлять полный интерфейс code review, обсуждения PR, комментарии, approvals, LFS, marketplace или организационное управление;
- оркестратор инфраструктуры и cluster manager: продукт может запускать delivery jobs, но не управляет кластерами, облачными аккаунтами или инфраструктурным inventory как системой учёта.

Минимальный встроенный bare Git hosting, просмотр репозитория, сравнение refs и ограниченные операции с pull request допустимы только как поддержка потока доставки. Они не расширяют границы Forge до платформы совместной разработки или полного code-review продукта.

## 5. Статусная таксономия

Каждое capability-утверждение в продуктовых и архитектурных документах использует один из следующих статусов:

- **Current verified** -- функция работает в текущем коде и подтверждена проверяемым evidence. Ограничения эксплуатации должны быть указаны рядом с утверждением.
- **Configuration only** -- пользователь может сохранить настройку или увидеть форму, но исполнительный процесс либо доставка результата отсутствуют. Это не считается работающей автоматизацией.
- **Target approved** -- требование принято ADR или нормативным контрактом, но ещё не реализовано. Его нельзя представлять как доступную функцию.

Статус отражает фактическую зрелость capability, а не наличие экрана, таблицы хранения или заготовки API. Актуальный снимок доказанных возможностей и ограничений находится в `docs/CURRENT_STATE.md`.

## 6. Capability-требования и приоритеты

Приоритет P0 означает обязательный результат для доверенного self-hosted контура; P1 -- необходим для безопасной и полноценной v1-эксплуатации; P2 -- последующее расширение без изменения границ продукта.

| REQ-ID | Capability | Приоритет | Статус | Требование к продукту |
|---|---|---:|---|---|
| REQ-PRJ-001 | Проекты и источник кода | P0 | **Current verified** | Пользователь может создать проект, связать его с внешним или встроенным Git-репозиторием и работать с выбранным ref. Встроенный Git поддерживает минимальный transport, достаточный для цикла push и запуска CI. |
| REQ-PIPE-001 | Определение pipeline | P0 | **Current verified** | Pipeline формируется из `.forge-ci.yml` в репозитории; при его отсутствии используется предсказуемый fallback. Current parser поддерживает legacy `stages/jobs` и v1 DAG MVP: `version: 1`, top-level `jobs`, `commands`, `needs`, `tags`, `secrets`, defaults `image/timeout/tags` и `allow_failure`; v1 пока исполняется через топологические runtime-стадии. |
| REQ-PIPE-002 | План pipeline как неизменяемое доказательство | P1 | **Current verified MVP** | Перед запуском конфигурация валидируется и фиксируется в `pipeline_plans` как immutable snapshot с raw config/fallback template, parser version, `config_sha256`, normalised plan JSON, `plan_sha256`, dependency edges и v1 `required_tags`/`required_secrets`/`artifact_paths`. Current форматы: `legacy-linear` и `v1-dag`; policy snapshot, variables в плане, line/column parser diagnostics, `on`/retry/`artifacts.expire_in` и job-level dispatcher остаются target. |
| REQ-EXEC-001 | Выполнение задач | P0 | **Current verified** | Embedded runner выполняет задачи в Docker либо shell-режиме, отражает жизненный цикл job и позволяет отмену/повтор в доступных границах. Этот режим предназначен для доверенного локального контура. |
| REQ-EXEC-002 | Внешний runner protocol и изолированные runner-ы | P1 | **Current verified MVP** | Control plane уже выдаёт работу через durable `job_queue` + runner protocol MVP: register/heartbeat/bounded long-poll `work:poll` с process-local + PostgreSQL `LISTEN/NOTIFY` wakeup/ack/control/`secrets:resolve`/artifact upload/renew/logs/complete, bearer runner credential, lease token, `workspace.checkoutUrl`, fencing generation, basic tag matching (`required_tags ⊆ runner.tags`), current `shell` executor capability matching, scoped secret delivery, declared artifact upload, ack-timeout requeue, configurable queue-timeout diagnostic при отсутствии compatible runner-а и stale-runner offline reconciliation. `forge-runner` даёт отдельный shell-runner process для checkout/command/secret env/artifact upload/stdout-stderr logs/active-lease heartbeat/cancel polling/terminal completion; production sandbox isolation, resource limits, pool/protected-tag policy, advanced capability matching, fairness и resumable artifact sessions остаются target, чтобы API-процесс не был production-исполнителем пользовательского кода. |
| REQ-EXEC-003 | Статусы выполнения | P0 | **Current verified** | Пользователь видит согласованные статусы pipeline, стадий и задач; переходы валидируются, итог агрегируется вверх, а active/latest `execution_attempt` синхронизируется с текущим status job. |
| REQ-EXEC-004 | Execution attempts, retry history и leases | P0 | **Current verified MVP** | Каждый запуск или повтор job создаёт отдельную неизменяемую попытку с собственными timestamps, terminal result, логами и metadata артефактов; retry не удаляет доказательства предыдущей попытки. Embedded runner фиксирует owner/expiry/outcome в `job_leases`; внешний runner protocol MVP создаёт active lease, выдаёт opaque lease token, requeue-ит unacknowledged offer после `ackDeadline`, завершает dispatch-eligible очередь после configured queue timeout при отсутствии compatible execution path и проверяет ack/renew/control/artifact upload/logs/complete по runner credential + fencing generation, а claim дополнительно фильтрует `job_queue.required_tags` и current `shell` executor compatibility. |
| REQ-OBS-001 | Логи | P0 | **Current verified** | Логи job append-only, упорядочены внутри `execution_attempt` и доступны для диагностики. Текущий просмотр использует polling и совместимый latest/open shortcut. |
| REQ-OBS-002 | Диагностические логи | P1 | **Current verified MVP** | Длинные логи доступны через bounded page/search (`limit`/`after`/`q`) и SSE stream; attempts хранят exit code, timestamps и error tail. Command span и stream classification остаются target. |
| REQ-ART-001 | Артефакты | P0 | **Current verified** | Пользователь может сохранить и получить артефакт job из локального хранилища в рамках текущего лимита размера; новые uploads получают metadata текущей/latest attempt. |
| REQ-ART-002 | Надёжное хранилище и retention артефактов | P1 | **Current verified MVP** | Новые артефакты получают SHA-256, `expires_at` по `CICD_ARTIFACT_RETENTION_DAYS` и download отвергает checksum drift/expired/purged записи. Retention worker удаляет expired local files и оставляет `purged_at` + audit evidence. Object storage adapter, tenant/object isolation, legal hold и resumable artifact sessions остаются target. |
| REQ-SEC-001 | Secrets | P0 | **Current verified** | Проектные секреты шифруются при хранении, их значения не возвращаются пользователю после сохранения, execution path выдаёт только job-declared names и маскирует известные значения в stdout/stderr best-effort. |
| REQ-SEC-002 | Scoped secret delivery и full redaction | P1 | **Current verified MVP** | Авторизованный runner получает только `jobs.required_secrets` на время active acknowledged lease через `secrets:resolve`; `forge-runner` inject-ит их в env и маскирует stdout/stderr. Full redaction во всех API/audit/error/trace каналах, KMS/rotation policy и environment-scoped secret policy остаются target. |
| REQ-ENV-001 | Environments и deployments | P1 | **Current verified** | Пользователь ведёт metadata окружений и append-only историю развёртываний для проекта. Текущий capability не означает автоматическую оркестрацию инфраструктуры вне pipeline. |
| REQ-ENV-002 | Approval и rollback delivery | P2 | **Current verified MVP** | Protected environment требует approval до запуска связанного pipeline, решения хранятся append-only, а rollback создаёт отдельную traceable deployment запись через новый pipeline. Расширенные policy rules, multi-approver workflow и rollback orchestration остаются target. |
| REQ-AUTO-001 | Git-события и автоматический trigger | P0 | **Current verified** | Push во встроенный Git может создать pipeline, связанный с изменённым ref. |
| REQ-AUTO-002 | Schedules и outgoing webhooks | P1 | **Current verified MVP** | Scheduler использует строгий 5-польный UTC cron, persisted `next_fire_at`, уникальный fire-slot и idempotent pipeline trigger; terminal pipeline events доставляются в enabled outgoing webhooks через basic outbox/retry/HMAC. IANA timezone/DST/misfire, scheduler leases и full delivery policy остаются target. |
| REQ-AUTO-003 | Надёжная automation delivery | P1 | **Current verified MVP** | Current MVP фиксирует outbox delivery attempts, terminal `failed_at`, bounded delivery history и явный requeue failed-доставки новой generation. Lease/fencing/crash recovery, full dead-letter operator policy и single observed outcome для всех async effects остаются target. |
| REQ-AUTO-004 | Local notifications (`in_app`/`sse`) | P1 | **Current verified MVP** | Пользователь может сохранить `in_app`/`sse` каналы и получить local notification history/stream на terminal pipeline events. |
| REQ-AUTO-005 | External notification adapters и inbound provider webhooks | P1 | **Target approved** | Email/Slack adapters и public Git provider webhook handlers исполняются только после реализации sender/handlers, signature validation и delivery evidence. |
| REQ-AUTH-001 | Identity, роли и API-токены | P0 | **Current verified** | Пользователи, роли, argon2id credentials, session-bound access JWT, refresh sessions с rotate/logout и scoped PAT хранятся и применяются при непустом `CICD_AUTH_SECRET`; без секрета действует trusted-network режим. |
| REQ-AUTH-002 | Project-scoped auth/RBAC enforcement | P1 | **Current verified MVP** | Project-owned API, name-based repo API и Git Smart HTTP read/write проверяют личность, route role, `project_memberships`, PAT `project_id` и scopes при непустом `CICD_AUTH_SECRET`. Tenant isolation, service-account tokens, scoped Git credentials и production cookie/CSRF/session-family policy остаются target. |
| REQ-AUD-001 | Audit и отчётность | P1 | **Current verified** | Forge хранит append-only audit entries и показывает базовые агрегаты по успешности и длительности pipeline. Эти данные не заменяют полноценную observability-платформу. |
| REQ-OBS-003 | Операционная наблюдаемость и восстановление | P1 | **Current verified MVP** | Оператор получает liveness, DB-aware readiness, metrics и локальный scripted backup/restore helper для PostgreSQL, Git и артефактов в Docker Compose. Off-site/PITR backup platform, регулярный restore drill, alert routing и reconciliation после перезапуска остаются target. |
| REQ-UI-001 | Dashboard, CLI и API | P0 | **Current verified** | Основные рабочие сценарии доступны через Dashboard, CLI и API; представления не должны объявлять target- или configuration-only функцию завершённой. |
| REQ-API-001 | Versioned совместимые контракты | P1 | **Current verified MVP** | Публичные контракты имеют committed OpenAPI 3.1 source of truth, generated frontend DTO, drift gate и backward compatibility diff с base/default branch. Полный cursor/idempotency contract, examples validation и major-version lifecycle остаются target. |

## 7. Нефункциональные требования

### Безопасность и доверие

- **NFR-SEC-01** До production-grade auth boundary Forge допускается только в доверенной локальной или изолированной сети; для shared deployment обязателен непустой `CICD_AUTH_SECRET`, reverse proxy/network boundary, непустой Git token и закрытый PostgreSQL.
- **NFR-SEC-02** Доступ к проектным данным, Git-операциям, артефактам, секретам и действиям доставки должен проверяться до загрузки или изменения данных.
- **NFR-SEC-03** Секреты передаются только через выделенные секретные каналы и конфигурацию; plaintext не сохраняется в логах, audit trail, ответах API или UI.
- **NFR-SEC-04** Credential classes пользователя, API automation, внутреннего worker и runner должны быть раздельны и иметь минимальные scopes.
- **NFR-SEC-05** Все изменяющие действия, имеющие административное или delivery-значение, оставляют неизменяемый audit след без чувствительных данных.
- **NFR-SEC-06** Зависимости, lockfile, CI actions, container images, committed secrets и SBOM управляются как часть trusted supply chain: current baseline блокирует known Rust/Node advisories, committed-secret patterns и drift SBOM, target дополняет это license/source policy, container/history secret scan и release SBOM artifact.

### Надёжность и целостность

- **NFR-REL-01** Pipeline plan, execution attempts, логи, metadata артефактов, deployment history и audit entries являются доказательствами и не переписываются задним числом; исправление создаёт новую запись. Current MVP закрывает immutable `pipeline_plans` snapshot для legacy и v1 DAG, retry history для attempts/logs/artifact metadata, checksum и local retention cleanup для новых artifacts; policy-aware plan и full object-storage lifecycle остаются target.
- **NFR-REL-02** Асинхронные эффекты допускают повтор доставки, но наблюдаемый итог остаётся идемпотентным. Current MVP покрывает pipeline trigger replay/conflict, embedded runner lease expiry reconciliation, local notification delivery и bounded outbox delivery history/requeue; full external lease recovery/crash retry для всех async effects остаётся target.
- **NFR-REL-03** Статус pipeline должен быть согласован с состоянием дочерних сущностей и не может обходить доменные правила переходов.
- **NFR-REL-04** Хранилища PostgreSQL, Git и артефактов имеют документированную и проверяемую процедуру backup/restore; запуск после сбоя восстанавливает согласованное рабочее состояние.
- **NFR-REL-05** Выполнение имеет явные timeout/cancel/recovery границы: current embedded runner хранит `job_leases` и fail-ит expired/missing owner; external runner protocol MVP добавляет heartbeat, lease token, ack/renew/control/artifact upload/logs/complete, `workspace.checkoutUrl`, fencing generation, basic tag + executor capability matching, ack-timeout requeue, configurable queue-timeout diagnostic без compatible runner-а, active-lease heartbeat, stale-runner offline reconciliation и отдельный `forge-runner` shell process. Target дополнительно требует production dispatch, лимиты ресурсов, sandbox isolation, расширенную lost-runner restart/race suite и отсутствие бесконечного `running` в production topology.

### Производительность и масштабирование

- **NFR-PERF-01** Типичные интерактивные операции Dashboard и API должны иметь измеримые SLO, определённые эксплуатационным контрактом; показатели не объявляются выполненными до нагрузочного evidence.
- **NFR-PERF-02** Списки и потоки данных имеют детерминированный порядок, pagination или cursor, лимиты запроса и ответа; система не загружает неограниченный объём логов или артефактов в память.
- **NFR-PERF-03** Ограничения на размер логов, артефактов, параллелизм и retention настраиваются и наблюдаемы оператором. Current MVP задаёт 50 MiB upload limit и `CICD_ARTIFACT_RETENTION_DAYS` TTL; quota/metrics/concurrency budgets остаются target.

### Совместимость и удобство

- **NFR-UX-01** Dashboard работает на desktop и mobile без потери ключевого сценария; управление с клавиатуры, видимый focus и семантические статусы обязательны.
- **NFR-UX-02** Пользовательские тексты поддерживают русский и английский; машинные идентификаторы и значения контрактов остаются стабильными.
- **NFR-API-01** CLI не зависит от прямого доступа к базе, filesystem сервера или внутренним implementation details.
- **NFR-API-02** Изменения публичных контрактов сохраняют обратную совместимость в активной версии либо проходят управляемую versioned migration.

## 8. Критерии приёмки по capability

### Проект, Git и pipeline definition

- Разработчик создаёт проект, связывает его с допустимым репозиторием, запускает pipeline для ref и в результате видит привязку запуска к исходному коду.
- Push во встроенный Git создаёт ровно один ожидаемый pipeline для события; повторная обработка одного события не создаёт неконтролируемых дубликатов.
- Pipeline из `.forge-ci.yml` воспроизводимо отображает определённые legacy стадии/задачи либо v1 DAG jobs/needs; fallback явно различим и не выдаётся за конфигурацию пользователя.
- Невалидная current-конфигурация не создаёт частично исполняемый pipeline; сохранённый plan можно соотнести с commit/ref и execution attempts. Target planner дополнительно должен сохранять diagnostics/policy evidence без enqueue executable jobs.

### Execution, статусы и логи

- Тестовая job действительно выполняет заданную команду в заявленной среде; успешный вывод приводит к terminal success и доступному логу, ошибочный -- к terminal failed с диагностикой.
- Cancel прекращает доступное выполнение и не позволяет ему позднее записать противоречивый terminal status.
- Недопустимый переход статуса отклоняется; агрегированный статус стадии и pipeline соответствует результатам дочерних jobs.
- Лог сохраняет порядок append-only записей. Целевой realtime-режим не пропускает либо не дублирует видимые записи при переподключении клиента.
- Повтор job или pipeline не удаляет старые логи и позволяет сравнить все attempts по времени, результату и диагностическому выводу.
- Для внешнего runner-а потеря lease или heartbeat приводит к наблюдаемому восстановлению либо безопасному завершению попытки, а не к бесконечной неопределённости.

### Артефакты и secrets

- Разрешённый пользователь может загрузить и скачать тестовый артефакт, а лимит размера отклоняет недопустимый upload без повреждённого metadata или файла.
- Артефакт одного проекта недоступен пользователю другого проекта после включения policy enforcement.
- Значение секрета не возвращается после создания, не появляется в UI, audit, ошибках и job logs.
- Embedded runner получает project secrets только в runtime env, а вывод, содержащий известное значение секрета, хранится в отредактированном виде.
- Политика retention удаляет просроченные артефакты предсказуемо и оставляет проверяемое operational evidence.

### Delivery и automation

- Пользователь может создать environment и зафиксировать deployment, связанный с pipeline; история не изменяется при последующих развёртываниях.
- Schedules, outgoing webhooks, bounded outbox delivery history/requeue и `in_app`/`sse` notifications помечены как **Current verified MVP** до появления IANA timezone/DST/misfire, lease/dead-letter семантики и внешних adapters; email/Slack adapters и inbound provider webhooks остаются target, пока соответствующие sender/handlers не исполняют доставку.
- Current automation событие или расписание создаёт ожидаемый результат в MVP-границах, delivery имеет наблюдаемый outcome, а failed delivery можно явно поставить в повтор без перезаписи исходной истории.
- Для protected delivery approval требуется до исполнения, а rollback создаёт отдельную traceable запись и не подменяет исходный deployment.

### Identity, governance и клиенты

- До получения **Current verified** для enforcement защищённый сценарий проверяет session или token, project membership и необходимую роль; viewer не может изменить проект, pipeline, секрет или policy.
- Отзыв токена либо отключение пользователя немедленно блокирует новые обращения в пределах определённой модели сессий.
- Административное, секретное и delivery-действие создаёт audit запись с субъектом, временем, объектом и безопасным описанием действия.
- Dashboard и CLI позволяют пройти P0-сценарий без обхода публичного контракта; UI чётко различает фактические, конфигурационные и целевые возможности.
- Изменение публичного поведения имеет контрактные тесты и не ломает поддерживаемых клиентов активной версии.

## 9. Out of scope

Следующее не входит в утверждённый объём Forge, если отдельное решение продукта не изменит настоящие требования:

- управление задачами, sprint planning, Kanban/Scrum-доски и issue templates;
- документация с совместным редактированием и публикация статических сайтов как CMS;
- OCI, npm, Maven, PyPI или иной registry, mirror и dependency proxy;
- браузерная IDE, редактор файлов, remote development environment, terminal для разработки и code intelligence как IDE-функция;
- полный code review: threaded comments, review approvals, merge queues, protected-branch governance и social collaboration вокруг pull request;
- управление Kubernetes, Terraform state, облачными ресурсами, inventory или secret manager как самостоятельный продукт;
- универсальная observability-платформа, SIEM, full-featured analytics или замена системы резервного копирования предприятия;
- мультитенантный SaaS control plane и обещания managed hosting.

## 10. Связанные источники

- `docs/CURRENT_STATE.md` -- проверенный текущий функциональный срез и ограничения.
- `docs/FUNCTIONAL_ARCHITECTURE.md` -- capability map, границы контекстов и инварианты.
- `docs/ROADMAP.md` -- порядок поставки утверждённых target capabilities.
- `docs/contracts/` -- нормативные наблюдаемые контракты.
- `docs/ADR.md` -- принятые архитектурные решения.
