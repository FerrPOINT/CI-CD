# Реагирование на инциденты Forge CI/CD

Этот документ определяет единый процесс реагирования на эксплуатационные и security-инциденты Forge CI/CD. Он не заменяет технические инструкции: действия по диагностике, восстановлению pipeline, runner, backup/restore и outbox описаны в [OPERATIONS.md](OPERATIONS.md).

> **Граница текущего MVP.** В Current verified доступны Docker health/status, `/api/v1/health`, `/api/v1/readiness`, read API, `/metrics` и структурированные логи. Alert routing, distributed runner pool и статус-страница относятся к Target approved. `/api/v1/health` подтверждает только liveness backend, а `/api/v1/readiness` проверяет PostgreSQL и SQLx migrations.

## Severity matrix

Уровень определяет Incident Commander (IC) по наибольшему фактическому или правдоподобному impact. При недостатке данных выбирается более высокий уровень до завершения triage. Время реакции -- максимальное время до подтверждения владельцем реагирования, а не время полного устранения.

| Уровень | Определение | Примеры для Forge | Время реакции |
|---|---|---|---|
| **SEV1** | Полная недоступность control plane, подтверждённая потеря данных либо активный/вероятный масштабный security compromise. | API и Dashboard недоступны для всех пользователей; утрачены PostgreSQL, Git или artifacts без подтверждённого восстановления; раскрыт master key или другой секрет с широким доступом; скомпрометирован основной Git-репозиторий или release signing identity. | До 15 минут, круглосуточно для production после его ввода. |
| **SEV2** | Существенно нарушена ключевая функция или есть security-инцидент с ограниченным, но значимым scope. | Пайплайн-кластер или embedded execution не выполняет большинство jobs; PostgreSQL недоступен при живом API; подтверждённая компрометация runner-а; утечка project/deploy token; значительная часть Git Smart HTTP операций не работает. | До 30 минут. |
| **SEV3** | Частичная деградация без подтверждённой потери данных и с доступным обходным путём. | Рост ошибок отдельных API-операций; недоступен Dashboard при работающем API; зависли отдельные jobs/пайплайны; заполнение диска, пока service продолжает работать; сбой конфигурации webhook/notification без гарантированной доставки. | До 4 рабочих часов. |
| **SEV4** | Низкий impact, локальная неисправность или риск без активного нарушения сервиса. | Одна неважная job завершилась с ошибкой; краткая деградация логов; предупреждение healthcheck без подтверждённого impact; документированная уязвимость без признаков эксплуатации. | До 2 рабочих дней. |

SEV1 и SEV2 требуют немедленного incident channel, назначения IC и регулярных обновлений. Если impact растёт или появляются признаки security compromise, IC повышает severity и фиксирует время изменения.

## Роли и ответственность

Один человек может совмещать роли только при небольшом инциденте; для SEV1 роли IC и технического исполнителя по возможности разделяются.

| Роль | Ответственность |
|---|---|
| **Incident Commander (IC)** | Объявляет и классифицирует инцидент, назначает роли, определяет приоритеты и безопасные решения, ведёт timeline, принимает решение о mitigation/resolution, обеспечивает postmortem. IC не обязан лично исправлять систему. |
| **Технический лид (Tech Lead)** | Руководит диагностикой и техническим восстановлением, собирает evidence, предлагает обратимые меры, назначает исполнителей и подтверждает технические критерии resolution. Не выполняет destructive recovery без решения IC и verified backup. |
| **Коммуникации (Comms Lead)** | Ведёт внутренние обновления, собирает подтверждённые факты у IC/Tech Lead, публикует согласованные сообщения и отмечает время/аудиторию. Не публикует секреты, персональные данные, внутренние IP, токены или непроверенную причину. |
| **Исполнители** | Выполняют конкретные назначенные действия, фиксируют команды, UTC-время, результат и новые риски в incident timeline. |
| **Владелец сервиса/безопасности** | Подтверждает бизнес-impact и привлечение внешних сторон; для security-инцидента утверждает scope, rotation и уведомления по применимым обязательствам. |

## Lifecycle

### 1. Detection

**Current verified:** мониторить Docker status/healthchecks, `GET /api/v1/health`, `GET /api/v1/readiness`, `pg_isready`, Dashboard, безопасный read API, `/metrics`, логи backend/PostgreSQL и состояние диска. Конкретные команды и интерпретация приведены в разделе «Мониторинг и health» [OPERATIONS.md](OPERATIONS.md#мониторинг-и-health). Health API не использовать как единственное доказательство работоспособности: отдельно проверять readiness, PostgreSQL и read API.

**Target approved:** централизованные structured logs, внешняя uptime-проверка и alert routing. Минимальные alert-и охватывают API/DB unavailable, error rate, disk pressure, backup age/failure, runner health, queue age, lease/reconciliation, outbox lag/dead letters, Git/artifact integrity и key/KMS availability. Метки и alert payload не должны содержать secret, token, tenant/project ID, URL либо event/delivery ID.

При сигнале создайте запись инцидента с UTC-временем, источником сигнала, затронутым сервисом и первичным impact. Сохраните `docker compose ps`, релевантные логи и release/commit до изменения системы; не копируйте в запись секреты или полный `.env`.

### 2. Triage

1. Назначить IC и severity, открыть внутренний incident channel.
2. Подтвердить impact: какие control-plane, Git, execution, storage или security-границы затронуты; затронуты ли данные, секреты, pipelines и пользователи.
3. Определить scope и начальную гипотезу по evidence, не выдавая гипотезу за root cause.
4. Принять безопасный план mitigation: остановка writers, draining/изоляция runner-а, rollback, restore или ограничение доступа. Для destructive действия сначала создать и проверить backup.
5. Установить период следующего обновления и владельцев задач.

Запрещено вручную переписывать статусы jobs/pipelines или записи БД SQL-командами ради «исправления» симптома.

### 3. Mitigation

Цель mitigation -- остановить рост impact и сохранить evidence, а не немедленно найти окончательную причину. Начинать с обратимых действий; destructive recovery требует явного решения IC.

Используйте соответствующие runbook-и в [OPERATIONS.md](OPERATIONS.md#инцидент-ранбуки), не дублируя их здесь:

- падение pipeline -- [Упавший pipeline](OPERATIONS.md#упавший-pipeline);
- зависшая работа -- [Зависший job](OPERATIONS.md#зависший-job);
- потеря или недоступность runner-а -- [Потерянный runner](OPERATIONS.md#потерянный-runner);
- очередь доставок -- [Outbox backlog](OPERATIONS.md#outbox-backlog);
- backup, restore и проверка целостности -- [Backup и восстановление](OPERATIONS.md#backup-и-восстановление).

Текущий MVP использует embedded runner, external runner protocol slice для register/heartbeat/poll/ack/renew/complete и `forge-runner` shell process. Не заявляйте, что это уже production distributed runner platform: durable queue, sandboxed runner, lost-runner auto-dispatch, inbound handlers и external notification adapters ещё не восстанавливают исполнение или доставку автоматически.

### 4. Resolution

Инцидент переводится в resolved, когда устранена непосредственная причина или безопасный обходной путь, impact прекратился и подтверждены необходимые проверки: health, PostgreSQL, read API, затронутый pipeline/Git/storage flow и отсутствие новых ошибок в согласованное окно наблюдения.

IC фиксирует UTC-время resolution, фактический impact, применённые изменения, остаточные риски и владельца дальнейших работ. Если причина ещё не подтверждена, статус остаётся resolved с явной пометкой «root cause pending», а не «закрыто без причины».

### 5. Postmortem

Для SEV1/SEV2 postmortem обязателен; для SEV3 выполняется при повторяемости, потере данных, security impact или по решению IC. SEV4 фиксируется как issue/risk, если не требует отдельного разбора. Postmortem должен быть blameless: он исследует условия, решения и системные барьеры, а не ищет виноватого.

## Шаблон postmortem

```markdown
# Postmortem: <краткое название>

- Incident ID: <id>
- Severity: <SEV1-SEV4>
- Status: <resolved / follow-up in progress>
- Incident Commander: <имя/роль>
- Tech Lead: <имя/роль>
- Comms Lead: <имя/роль>
- Начало (UTC): <timestamp>
- Resolution (UTC): <timestamp>

## Impact

- Затронутые сервисы/пользователи/проекты: <scope>
- Фактический impact: <недоступность, задержка, потеря данных, security impact>
- Длительность и данные, которые могли быть затронуты: <details>

## Timeline (UTC)

| Время | Событие / наблюдение | Решение и владелец |
|---|---|---|
| <timestamp> | <факт> | <действие> |

## Root cause и contributing factors

- Root cause: <подтверждённая техническая причина>
- Contributing factors: <контекст, отсутствующие барьеры, условия>
- Что сработало: <детектирование, mitigation, recovery>
- Что не сработало: <пробелы в процессе или технике>

## Action items

| Действие | Owner | Приоритет | Срок | Статус |
|---|---|---|---|---|
| <конкретное проверяемое улучшение> | <роль/имя> | <P0-P2> | <date> | <open/in progress/done> |

## Evidence и коммуникации

- Ссылки на incident timeline, safe logs, dashboards, backup/restore evidence: <links>
- Внутренние/внешние сообщения и время публикации: <links/timestamps>
```

## Security-инциденты

Для любого подозрения на security compromise сначала ограничьте распространение, сохраните безопасное evidence и поднимите severity до SEV1 или SEV2 до подтверждения scope. Не размещайте секреты, токены, приватные ключи, вредоносные payload или полные forensic-образы в обычном incident channel.

### Утечка секрета

1. Немедленно прекратить использование раскрытого секрета: отозвать его у issuer/provider, а не только удалить из Forge или `.env`.
2. Заблокировать связанные principal, API token, deploy key, webhook signing secret или service account; для master key оценить необходимость emergency recovery/перешифрования данных.
3. Сгенерировать новый секрет в утверждённом secret manager/защищённом канале, минимизировать scope и срок действия.
4. Обновить все разрешённые consumers, выполнить controlled rollout и проверить, что новый секрет работает, а старый отклоняется.
5. Найти и удалить секрет из Git history, логов, artifacts, тикетов и кэшей по утверждённой процедуре; удаление не заменяет revocation.
6. Проверить audit/log evidence на использование секрета, определить scope, уведомить владельца и зафиксировать rotation в postmortem.

Current verified хранит project secrets шифрованными AES-256-GCM, инъецирует их в env embedded runner, маскирует stdout/stderr best-effort, применяет project memberships и scoped PAT при включённом `CICD_AUTH_SECRET`. Однако auth/RBAC остаётся условным, tenant isolation, service-account/scoped Git credentials и target redaction/rotation ещё не завершены; shared deployment с текущим MVP не считается безопасной production-конфигурацией.

### Компрометация runner-а

1. Немедленно изолировать runner/host от сети и перевести его в disabled/draining; запретить новые задания. Не использовать его для диагностики других инцидентов.
2. Сохранить доступное безопасное evidence: время, runner ID/host, последние jobs, образ/commit, логи и сетевые наблюдения. Не доверять данным с потенциально скомпрометированного host как единственному источнику.
3. Отозвать и ротировать все доступные runner-у credentials: registration/lease/API tokens, Git/deploy credentials, secret bundles и cloud/container credentials. Рассматривать их как раскрытые.
4. Остановить или отменить затронутые executions согласно runbook; проверить возможные side effects и не повторять deploy автоматически без оценки идемпотентности.
5. Пересоздать runner из известного доверенного образа/host, зарегистрировать с новыми scoped credentials и допустить обратно только после проверки security owner.
6. Оценить lateral movement: доступ к Git, artifacts, control plane, Docker daemon, сети и secrets; расширить scope/уведомления при необходимости.

В Current verified embedded runner находится в backend и использует Docker или host shell. Его compromise рассматривается как compromise backend host и требует эскалации не ниже SEV2; для production целевая граница -- отдельный runner process без Docker socket у control plane, как определено в [ADR-0007](adr/0007-runner-security-boundary.md).

### Компрометация Git-репозитория

1. Приостановить affected Git ingress, deployments и автоматические pipeline triggers; сохранить SHA, refs, audit/log evidence и список затронутых репозиториев.
2. Защитить известную good revision: зафиксировать commit/tag, проверить владельца/подпись там, где это настроено, и не перезаписывать evidence force-push-ем.
3. Отозвать и ротировать credentials, имевшие write-доступ: Git tokens, deploy keys, webhook/internal hook tokens, CI credentials и signing keys при риске их доступа.
4. Определить malicious commits, изменённые refs, pipeline definition и выпущенные artifacts/deployments; считать `.forge-ci.yml` и pipeline output недоверенными до проверки.
5. Восстановить refs только через согласованную защищённую процедуру и known-good commit; затем проверить Git integrity и повторно запустить только безопасные, оценённые pipelines.
6. Провести review доступа, branch protection, hook configuration и downstream потребителей; оформить postmortem и обязательные action items.

## Коммуникации

### Внутренние каналы

IC создаёт выделенный incident channel в назначенном внутреннем канале инженерии/эксплуатации и добавляет владельца сервиса и security owner при security impact. Для SEV1/SEV2 Comms Lead публикует начальное сообщение после назначения IC, затем обновления по согласованному интервалу и финальное resolution-сообщение.

Каждое сообщение содержит: incident ID, severity, начало в UTC, подтверждённый impact, затронутые компоненты, текущие действия, известный безопасный workaround и время следующего обновления. Формулировки отделяют факты от гипотез. Не включать секреты, токены, персональные данные, внутренние адреса или детали, которые помогают атакующему.

### Статус-страница

**Target approved:** внешняя статус-страница с history инцидентов и компонентами API, Dashboard, Git hosting, pipeline execution, artifacts и notifications. Только Comms Lead или назначенный заместитель публикует внешние сообщения после подтверждения IC. До появления статус-страницы внешние уведомления выполняются только через утверждённый канал владельца сервиса и фиксируются в incident timeline.

Минимальный внешний шаблон: «Расследуем / определили / устранили», время UTC, затронутый компонент, наблюдаемый impact, доступный workaround и ожидаемое время следующего обновления. Не публиковать неподтверждённую root cause или security-детали до согласования с security owner.

## Связанные документы

- [OPERATIONS.md](OPERATIONS.md) -- текущие проверки, восстановление и incident runbook-и.
- [SECURITY.md](SECURITY.md) -- текущие security-границы и планируемые controls.
- [CURRENT_STATE.md](CURRENT_STATE.md) -- фактически реализованные capabilities.
- [Контракт runner protocol](contracts/RUNNER_PROTOCOL.md) -- целевые leases, heartbeats и fencing.
- [Контракт событий и доставок](contracts/EVENT_CONTRACT.md) -- целевые outbox, delivery и replay.
