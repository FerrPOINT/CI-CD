# Полный набор проверок синхронизации Forge

**Актуализировано 2026-10-20:** переносимый план проверки результирующего merge-кандидата. Фактические результаты и source SHA публикуются отдельно после его gates; этот документ не заявляет свежий PASS.

Объём: согласованная первая версия GitHub.com и собственного GitLab. Это набор обязательных проверок, а не утверждение, что все они уже пройдены. Итог каждого подкейса фиксируется отдельно для провайдера, исходного tree, запуска и evidence.

## Матрица исполнений

Каждый применимый Git/API/worker/CI сценарий выполняется на GitHub и GitLab. Базовый обмен: pull/push/both × manual/automatic × 2 providers, плюс disabled/manual defaults. Проверки реальных эффектов используют независимые refs/SHA и журнал, а не только статус API. Все внешние изменения ограничены собственными refs существующих разрешённых QA-репозиториев.

Проверка O-RUNTIME (установка кандидата в принятый runtime) не входит в обязательную тестовую матрицу этого документа. Установка остаётся отдельным этапом цели; её отсутствие не объявляется ни выполненной установкой, ни отказом функционального теста. Статусы и объём 108 подкейсов этим примечанием не меняются.


## Границы проверки

Исходные 50 ID и 108 подкейсов ниже сохранены. Domain tests проверяют таблицу направлений и ancestry; normal PostgreSQL tests — durable store, авторизацию, HTTP, версионирование, идемпотентность, server filters и точный CI config SHA. Отдельные native Git и live opt-ins проверяют настоящий HTTPS, реальные refs/SHA, защиту, расписание и физические границы отказов. Frontend component tests проверяют пользовательские действия и состояние; transport tests проходят typed client с контролируемым fetch. Preview tests не заменяют production UI. Число запусков и coverage counters не подменяют эти oracle.

Два ограничения сохраняются отдельными scope, без сокращения требований: private branch protection GitHub требует возможностей текущего приватного QA-репозитория; первоначальный push в действительно пустой внешний репозиторий требует такого разрешённого endpoint. Ошибка 403 или непустой fixture не считается успешной проверкой. Новые репозитории, очистка исходных веток, смена visibility или тарифа не являются неявным шагом теста. Положительный GitHub read-only oracle и естественное окончание job token проверяются отдельно, используя соответствующие selectors; это не подмена protection или true-empty сценария.

Условия внешних opt-ins и повторяемые команды приведены после матрицы. Точные логи, версии и ограничения публикуются в merge-qualification отчёте и приложенных CI/PR artifacts с SHA inventory; checkout не должен зависеть от файлов вне репозитория.

## Обязательные сценарии

### F-01

Создать подключение из общего списка; выбрать доступный hosted repository.

**Ожидается:** До сохранения записи нет; после — одна запись по UUID, disabled/manual, encrypted credential. Existing external catalog entry не преобразуется.

Проверяемые части:

- Новое подключение с версией 0 через настоящий HTTP; начальные значения disabled/manual и DTO, раскрывающий только наличие credential.
- Выбор hosted-репозитория без сохранения и форма с обязательным credential; сохранение версии 0 через frontend.

### F-02

GET/PUT/check/start/history/run/refs, valid и invalid DTO.

**Ожидается:** HTTP/OpenAPI совпадают; check не меняет settings/version/refs; start 202+UUID; disabled start 409; worker off 503.

Проверяемые части:

- Валидация GET/PUT/check/start/run/refs и прав, отражённых в DTO.
- Успешная проверка несохранённого черновика не меняет Git и настройки.

### F-03

Два редактора сохраняют одну version.

**Ожидается:** Один успех, второй 409; draft второго остаётся; обновление версии не стирает edits; повтор с новой version работает.

Проверяемые части:

- CAS по expected_version допускает одного победителя и отклоняет устаревшие записи; команды идемпотентны.
- Настоящие ответы 200/409 в двух редакторах; черновик пользователя сохраняется после обновления версии.

### F-04

Потерять ответ PUT или POST runs и повторить прежний Idempotency-Key.

**Ожидается:** Одна settings version / один run; прежний readback. Другой payload, actor или repository с тем же ключом — 409.

Проверяемые части:

- Повтор той же операции с тем же payload возвращает сохранённый результат; другой payload вызывает конфликт.
- При неизвестном результате сохранения или запуска интерфейс блокирует изменения и повторяет прежний ключ операции.

### F-05

Поменять token, endpoint, provider, repository_path; отключить и включить.

**Ожидается:** Omitted token сохраняется, empty — 400; новый provider/instance требует token; вся история содержит исходные snapshots и UUID запусков.

Проверяемые части:

- Отсутствующий или пустой токен и его ротация; снимки endpoint сохраняют историю.
- Смена провайдера или origin требует токен; при возврате к прежним значениям токен можно снова не передавать.

### F-06

Менять настройки при queued/running/reconciling.

**Ожидается:** Запрещённые изменения — 409. Отключение отменяет queued, запрашивает stopping активного; история сохраняется.

Проверяемые части:

- Изменения настроек при queued или активном запуске вызывают конфликт; отключение сохраняет историю.
- Отключение до эффекта и после принятого эффекта; правдивые состояния stopping и окончательный результат.

### G-01

Equal branches/tags, повтор успешного запуска, disabled.

**Ожидается:** No-op; SHA, HEAD и default branch не меняются; disabled не выполняет передач.

Проверяемые части:

- Одинаковые refs дают no-op; повторный запуск и disabled; HEAD не переписывается.
- Настоящие пустые репозитории сохраняют HEAD Forge при первоначальном заполнении.

### G-02

Внешний → Forge: пустой Forge, новая ветка, FF и внешний ref отсутствует.

**Ожидается:** Создание/FF только внутрь; локальные refs без внешнего аналога сохраняются; внешние refs не меняются.

Проверяемые части:

- Только pull: новый ref, fast-forward, сохранение refs без источника; внешний репозиторий не меняется.
- Первоначальный pull в действительно пустой репозиторий Forge.

### G-03

Forge → внешний: пустой remote, новая ветка, FF и локальный ref отсутствует.

**Ожидается:** Создание/FF только наружу; refs назначения без источника сохраняются.

Проверяемые части:

- Только push: новый ref, fast-forward; ref назначения без источника сохраняется.
- Первоначальная публикация в действительно пустой внешний репозиторий.

### G-04

Both: Forge отстаёт, remote отстаёт; независимые новые ветки с двух сторон.

**Ожидается:** По каждому ref продвигается отстающая сторона; итоговые SHA равны.

Проверяемые части:

- В режиме both продвигается отстающая сторона и создаются независимые refs в обоих направлениях.

### G-05

Diverged branch и попытка отката в каждом направлении.

**Ожидается:** Conflict; обе исходные истории и SHA сохранены; нет merge/rebase/force overwrite.

Проверяемые части:

- Расхождение веток и попытки отката сохраняют обе истории; merge, rebase и перезапись не выполняются.
- Полная таблица направлений и сочетаний ancestry/refs на уровне domain.

### G-06

Annotated/lightweight tags; одинаковый peeled commit, разные tag objects; blob/tree tags.

**Ожидается:** Сохраняется точный tag-object SHA; существующий другой tag — conflict; blob/tree передаётся без CI.

Проверяемые части:

- Аннотированные и легковесные теги, конфликт точного объекта тега, тег на blob; окончательная причина отказа CI для объекта, не являющегося коммитом.
- Импорт аннотированного тега в пустой репозиторий.

### G-07

Удалить branch/tag на одной стороне; добавить provider/PR/internal refs.

**Ожидается:** Удаление не распространяется; внутренние refs не переносятся; HEAD не меняется. В both отсутствующий ref может снова создаться из сохранившейся стороны.

Проверяемые части:

- Удаление не распространяется; both восстанавливает отсутствующий ref; внутренние refs и refs PR исключены; HEAD не меняется.

### G-08

Защищённая ветка Forge: создание и FF; свободная feature рядом.

**Ожидается:** Protected ref не изменён, reason protected_branch, указан обычный PR; feature передана, общий итог partial.

Проверяемые части:

- Защищённые существующие и новые refs Forge блокируются, свободные refs применяются; общий результат partial.
- Ветка по умолчанию связанных проектов защищена независимо от явного списка.

### G-09

Защищённые refs на GitHub/GitLab, read-only token, правила workflow files.

**Ожидается:** Провайдер окончательно решает реальный push; отказ виден по ref, другие refs продолжаются. Dry-run не выдаётся за гарантию реальной записи.

Проверяемые части:

- Реальный отказ внешнего GitLab при записи в защищённую ветку.
- Один и тот же GitLab credential с минимальными правами только чтения позволяет читать, но реальный push запрещён.
- Отказ GitHub при записи в защищённую ветку приватного репозитория.
- Один и тот же GitHub credential успешно читает, затем получает отказ реальной записи.
- Передача точных байтов workflow GitHub без нежелательного запуска Actions.

### G-10

Конкурентный внешний push между наблюдением и публикацией; конкурентные receive-pack и PR merge в Forge.

**Ожидается:** Ожидаемый old SHA проверяется атомарно; чужое изменение не затёрто. Sync/receive/merge сериализованы по одному guard. Проверить также branch/tag с одинаковым коротким именем.

Проверяемые части:

- Конкурентное изменение внешнего ref отклоняется через CAS с ожидаемым SHA.
- Настоящие receive-pack и PR merge используют общий mutation guard и сигнализируют планировщику.
- Конкурентный PR merge создаёт один коммит; одинаковые короткие имена ветки и тега сохраняют семантику ветки.

### A-01

Manual mode, local push, PR merge и ожидание более 5 минут.

**Ожидается:** Автозапусков нет; manual работает.

Проверяемые части:

- Ручной режим игнорирует автоматические сигналы; ручной запуск доступен.
- Поведение очереди при disabled и ручном режиме.

### A-02

Включить automatic и не нажимать запуск.

**Ожидается:** Сохранение не делает скрытый push; первая периодическая проверка через interval. Проверить настоящий production interval 300 секунд, а не только ускоренный fixture.

Проверяемые части:

- Включение automatic назначает первый запуск через настоящий интервал; сохранение не выполняет скрытый push.

### A-03

Автоматический запуск по local receive-pack и по PR merge; затем потерять сигнал.

**Ожидается:** Сигнал даёт ближайший запуск по принятой политике направления; periodic восстанавливает работу без сигнала. Входящий обмен проверяется также без provider webhooks.

Проверяемые части:

- Настоящий локальный push и PR merge сигнализируют об автоматическом запуске.
- Периодическая сверка обнаруживает внешние изменения без webhook провайдера.
- Сигналы охватывают каждое включённое направление, включая pull.

### A-04

Много сигналов до/во время запуска, два worker-процесса.

**Ожидается:** Один active run на repository, максимум одна следующая сверка от накопленных сигналов; общий concurrency не выше 2.

Проверяемые части:

- Два владельца и fencing при claim, один активный запуск на репозиторий, объединение сигналов и слоты.
- Отдельные процессы ОС не обходят guard живого осиротевшего Git-процесса.

### A-05

Три и более repository, занятые guard/slots, перезапуск worker, изменение interval/concurrency.

**Ожидается:** Другие репозитории продвигаются; slots освобождаются; один репозиторий не исполняется параллельно; новая конфигурация валидируется.

Проверяемые части:

- Занятые guard первых двух репозиториев не блокируют продвижение третьего; слоты освобождаются.
- Начальные значения и ограничения интервала и параллелизма в типизированной runtime-конфигурации.
- Отдельный перезапущенный worker продолжает восстановленный запуск после освобождения guard.

### A-06

Архив до queue, между selection/admission и во время работы; разархивирование.

**Ожидается:** Новая запись приостановлена, история доступна; уже начатый эффект корректно сверяется; после восстановления работа продолжается без duplicate.

Проверяемые части:

- Архивирование блокирует очередь и команды, сохраняя историю; разархивирование допускает следующего кандидата.
- Гонка архивирования между выбором и admission, а также взаимное выполнение архивирования и отдельных фаз живого Git и восстановления.

### CI-01

Принять A, затем ветка стала B до доставки события A.

**Ожидается:** Pipeline/config/checkout привязаны к commit A; не к более позднему head.

Проверяемые части:

- SHA события остаётся исходным, даже если head ветки продвинулся.
- Настоящее событие провайдера использует точный SHA принятого коммита.

### CI-02

Повтор доставки; два worker; общий receipt с обычным push.

**Ожидается:** Ровно один pipeline на event key. После сбоя доставки событие остаётся и повторяется.

Проверяемые части:

- Общий push receipt и повторное событие создают один pipeline.
- Повторный обычный входящий push и событие синхронизации обрабатываются без дубликатов.
- Новый физический сбой после receipt последнего ref с проверкой отсутствия дубликатов CI.

### CI-03

Push-policy disabled или выбрана одна из нескольких CI configurations; tag без commit.

**Ожидается:** При disabled — 0 pipelines; иначе выбран правильный project; blob/tree tag имеет терминальный ci_non_commit_tag.

Проверяемые части:

- Отключённая push-policy и выбор конфигурации сохраняют идентичность решения при повторе.
- Тег на blob или tree не создаёт pipeline; сохраняется окончательная причина решения CI.

### CI-04

Both с обратными сигналами и повтором одинаковых SHA.

**Ожидается:** Система выходит в no-op; нет бесконечных запусков или дубликатов pipelines.

Проверяемые части:

- Both сходится к no-op при одинаковых SHA, накопленные сигналы объединяются без бесконечных запусков.
- Объединение сигналов соблюдает ограничение одного активного запуска.

### R-01

Worker умер до claim / после claim, до записи плана.

**Ожидается:** Нет потерянной работы и лишнего эффекта; lease recovery продолжает запуск.

Проверяемые части:

- Сохранённая работа в очереди, claims и fencing по generation переживают смену владельца.
- Отдельные физические остановки worker до claim и после claim до записи плана; каждая фаза проверяется самостоятельно.

### R-02

Intent сохранён, push ещё не начат.

**Ожидается:** При old SHA допустим безопасный повтор; нет слепой перезаписи.

Проверяемые части:

- Физическая остановка после сохранённого intent до реального push; перед безопасным повтором наблюдается прежний SHA.

### R-03

Удалённый сервер принял push, ответ потерян до receipt.

**Ожидается:** Получатель сохраняет new SHA; recovery отмечает applied по реальному наблюдению; второго изменения нет. Требуется настоящий обрыв, а не только ручное создание unknown journal.

Проверяемые части:

- Настоящий провайдер принимает push; worker останавливается до receipt, пока реальный дочерний процесс удерживает вывод и guard; эффект не повторяется.

### R-04

После применения части refs / после receipt последнего ref, до terminal run.

**Ожидается:** Уже принятые refs сохраняются; остальные доводятся или получают отдельный отказ; truthful общий итог.

Проверяемые части:

- Физическая остановка после первого сохранённого receipt, второго принятого push с неизвестным результатом и до третьего; восстановление каждого ref.
- Физическая остановка после receipt ПОСЛЕДНЕГО ref до окончательного состояния запуска.

### R-05

Supervisor умер, Git/HTTPS helper жив, lease истёк; запущен второй worker.

**Ожидается:** Живой child удерживает guard. Второй worker не пишет; после завершения child сверяет реальное назначение. Повторить между отдельными OS-процессами.

Проверяемые части:

- Прежний worker завершён, Git helper жив, lease истёк; второй процесс ОС заблокирован унаследованным guard.
- Сохранность mutation lock при завершении native helper или его родительского процесса.

### R-06

Provider недоступен при восстановлении unknown; затем доступен.

**Ожидается:** Reconciling, отсутствие нового push до наблюдения; после восстановления new/old/other SHA дают applied/safe retry/conflict соответственно.

Проверяемые части:

- Неизвестный эффект при недоступном провайдере остаётся reconciling; после восстановления перед повтором проверяется ожидаемый или прежний SHA.
- Иной наблюдаемый SHA назначения вызывает конфликт и не перезаписывается.

### R-07

Отключить во время fetch, push, после принятия push, при timeout.

**Ожидается:** Stopping до подтверждения; принятый push не становится ложным canceled. Секреты и история сохраняются.

Проверяемые части:

- Отключение до push предотвращает эффект.
- Отключение после принятого push сохраняет фактически принятый результат.
- Отдельные отключения во время fetch и на фазах timeout.

### R-08

Потеря DB/heartbeat/fence/slot во время Git.

**Ожидается:** Старый owner не подтверждает чужой generation; OS guard исключает параллельную запись; journal восстанавливается.

Проверяемые части:

- Физическое завершение и перезапуск PostgreSQL сохраняют intent с неизвестным результатом и возможность повтора операции.
- Истёкший heartbeat и fencing запрещают фиксацию устаревшему владельцу.
- Физическая потеря БД, heartbeat, namespace admission и slot во время живого Git-эффекта. Проверяются потеря admission до отправки и после принятого push, фактический пик живых Git-процессов при concurrency=2 и продвижение ожидающего третьего репозитория, а также SIGKILL/перезапуск собственного PostgreSQL при живом Git. Синтетический unknown journal не заменяет последний сценарий.

### R-09

Crash API во время check; disk full; Git missing/error/output limit; scratch cleanup failure.

**Ожидается:** Нет изменения live refs или секретов в артефактах; bounded завершение, безопасная диагностика; orphan scratch обнаруживается и убирается без чужих данных.

Проверяемые части:

- Физическое падение API во время check при живом дочернем процессе и восстановление осиротевшего scratch без изменения refs и настроек.
- Настоящий ENOSPC и ограниченное по времени восстановление; отрицательные проверки реального TLS-адаптера.
- Отсутствующий Git, timeout/cancel, унаследованные pipes, ограничения вывода, безопасная очистка scratch с сохранением чужих symlinks.

### R-10

Crash после создания pipeline до event_delivered; несколько повторов доставки.

**Ожидается:** Общий receipt возвращает прежний pipeline; event становится delivered; отказ CI не повторяет Git effect.

Проверяемые части:

- Общий CI receipt с точным SHA и повтор события предотвращают создание дубликата pipeline.
- Физическая остановка после создания pipeline до event_delivered, затем перезапуск и повтор.

### S-01

Anonymous, viewer/developer/admin, membership другого project, namespace active/archived, unbound legacy repo.

**Ожидается:** Права соответствуют действующей модели Forge; GET/history не расширяют write, чужой project binding не открывает repo. Полный HTTP middleware обязателен.

Проверяемые части:

- Полный auth middleware: анонимный доступ, viewer/developer/admin, членство в проекте и namespace admission.
- Настоящий legacy-вход viewer: чтение разрешено, запись, check и start запрещены.
- Область доступа при браузерном SSO.

### S-02

Machine token: правильный/чужой/отсутствующий project; api read/write и git write по отдельности; expired/revoked/disabled service account.

**Ожидается:** Разрешены только нужные операции своего project. REV-01 закрыт; store-only fixture недостаточен.

Проверяемые части:

- Machine scopes и привязка проекта, истёкшая, отозванная и отключённая учётная запись проверяются через настоящий auth middleware.

### S-03

Omitted/empty/invalid/expired/revoked/read-only provider credential; смена провайдера/инстанса; потерянный ответ сохранения.

**Ожидается:** Понятный отказ без plaintext; сохранённый token не раскрывается; корректная ротация и идемпотентный replay.

Проверяемые части:

- Отсутствующий или пустой токен, ротация и новый endpoint; fingerprints и возвращаемые результаты не раскрывают секреты.
- Проверка неверного credential без сохранения; отказ аутентификации.
- Поведение отозванного токена GitLab и токена с минимальными правами только чтения.
- Успешное чтение GitHub и реальный отказ записи с тем же credential только для чтения.

### S-04

GitLab read_repository/write_repository без api scope; GitHub выбранный repo, Contents/Workflows.

**Ожидается:** Обмен требует только достаточных Git permissions, отказ избыточного/недостаточного доступа проверен реальным push.

Проверяемые части:

- GitLab read_repository/write_repository без api scope; отказ после отзыва.
- Успешная передача Contents/Workflows GitHub с существующим credential.
- Отказ записи GitHub для выбранного репозитория при правах только чтения.

### S-05

Запрещённый origin, HTTP, credentials/query/fragment в URL, path traversal/encoding, backslash/userinfo и различия разбора Rust/Git, redirect на разрешённый и запрещённый origin.

**Ожидается:** REV-04 закрыт; фактический Git endpoint соответствует проверенному origin; redirects не следуются; token не уходит на другой endpoint.

Проверяемые части:

- Канонический разбор URL/origin; отклонение недопустимых userinfo, backslash, управляющих символов, пути, порта и базового пути.
- Настоящий GitSession отклоняет redirects и сохраняет проверенную TLS authority endpoint.
- Отрицательная проверка запрещённого origin через установленный API.

### S-06

Неверный TLS/hostname/expired certificate, private CA в системном trust store.

**Ожидается:** TLS не отключается; полный GitSession/API/worker повторяет ожидаемый результат, а не только внешний Git CLI.

Проверяемые части:

- Настоящий GitSession отклоняет недоверенный сертификат, неверный hostname и истёкший сертификат, принимает системный частный CA.
- Самостоятельный полный сценарий HTTP → worker с TLS и собственным CA.

### S-07

Ответы GET/ошибки, idempotency readback/HMAC, process argv/env, scratch config/objects, logs, frontend storage/URL, exports/build contexts/Git objects.

**Ожидается:** Точные canary credentials отсутствуют. Ciphertext в DB — ожидаемое исключение; plaintext не попадает jobs/runners/project secrets. Проверить и при аварийном завершении.

Проверяемые части:

- Семантика ciphertext, HMAC, возвращаемого результата, отсутствующего токена и очистки ошибок от секретов.
- Точные предоставленные секреты отсутствуют в проверенных исходниках, Git и содержимом всех сохранённых архивов evidence.
- Проверка секретов нового итогового кандидата и вновь созданных доказательств сбоев.

### S-08

CORS разрешённого/чужого origin, PUT preflight, malformed UUID/header/pager/filter.

**Ожидается:** REV-03 закрыт; invalid запрос даёт 4xx, не 500; разрешённый browser-origin может сохранять.

Проверяемые части:

- Настоящий CORS preflight PUT для разрешённого origin; отклонение чужого origin и неверных UUID, заголовка, параметров пагинации и фильтра.

### U-01

Более 40 connections и available repositories, оба provider, несколько namespace, одинаковые имена.

**Ожидается:** Search/provider/state/group filters, offset/limit/total вычислены сервером; доступны поздние страницы; выбран правильный UUID.

Проверяемые части:

- Серверная пагинация и фильтрация больших каталогов подключений, истории и refs.

### U-02

Все проекты ↔ конкретный проект; deep link, Back/Forward, вкладки, filters, run/ref offsets.

**Ожидается:** URL восстанавливает контекст; ссылка строки содержит настоящий namespace; чужой namespace не открывает editor. Токен/draft не появляются в URL/storage.

Проверяемые части:

- Настоящая навигация с URL-фильтрами, вкладками и контекстом проекта, переходами Back/Forward.
- Идентичность namespace и скрытие чужой области доступа.

### U-03

Создание и редактирование, четыре направления, manual/automatic, ручной запуск при automatic, read-only.

**Ожидается:** Сохраняется настоящее состояние API, без fixture transport; недоступные команды заблокированы и объяснены.

Проверяемые части:

- Создание и редактирование четырёх направлений, ручной и автоматический режимы, обязательные поля, начальные значения и доступ только для чтения.
- Настоящее изменение через API и ручной запуск при автоматическом режиме; сохранённое состояние провайдера.

### U-04

Более 20 runs и 50 refs; быстрый terminal, stopping, reconciling, paused archive/storage.

**Ожидается:** История и refs имеют окончательные статусы без reload; причины protected/access/conflict/unknown понятны; REV-02 закрыт.

Проверяемые части:

- Настоящие 61/68 refs и страницы истории; обновление окончательного состояния без перезагрузки.
- Версии снимков сохраняются после изменения настроек, настоящие исторические запуски загружаются.
- Отображение queued, stopping, reconciling и приостановки.

### U-05

Ошибка первого GET и polling, CAS 409, потерянные ответы PUT/POST, повторное нажатие.

**Ожидается:** Ошибка не превращается в пустой список; draft сохранён; повторяется прежний command ID; controls восстанавливаются.

Проверяемые части:

- Настоящий отказ первого или фонового GET отличается от пустых данных; черновик и кэш сохраняются; повтор восстанавливает работу.
- Настоящий CAS и неизвестный результат сохранения без сети сохраняют черновик и операцию повтора.

### U-06

375/768/1026/1440 px, keyboard only, labels, focus, busy/error states, длинный URL/ref.

**Ожидается:** Нет недоступных действий и горизонтального разъезда формы; компактный верх; fixture screenshots дополнены реальной страницей.

Проверяемые части:

- Настоящие ширины 375/768/1026/1440 и управление клавиатурой, длинные refs/URL, состояния занятости и ошибки.

## Дополнительные регрессии обнаруженных границ

- Физическое падение до claim и после claim до плана; повтор новым OS worker без лишнего эффекта.
- Падение после последнего ref receipt до terminal run; все receipts сохраняются, повторного push/CI нет.
- Падение после commit pipeline до event_delivered; сохраняются UUID pipeline и точный SHA.
- Archive выигрывает admission; archive ждёт уже допущенные fetch/push; orphan child и восстановление после принятого неизвестного эффекта.
- Disable во время настоящего fetch: stopping до завершения child, отсутствие записи и сохранность credentials/history.
- Потеря namespace admission connection перед push и после его принятия: отмена child, reconciling, независимая сверка SHA, отсутствие повторной записи.
- Потеря активной heartbeat connection: отмена Git без отпускания gate, восстановление того же intent новым owner.
- Потеря глобальной slot connection: максимум два одновременно живых Git исполнения на общем хранилище; третий repository продвигается после освобождения слота.
- Интерфейс показывает новую причину reconciling на русском; unknown ref не отображается как переданный.

## Gates и доказательства

Rust 1.88.0: fmt, all-target/all-feature Clippy с -D warnings, workspace tests, normal PG suite и обязательные opt-ins, release build. Node 22.20.0/pnpm 10.28.1: frozen install, tests, lint, typecheck, production build. OpenAPI drift/compatibility относительно закреплённого main и независимые Base SDK/UI pins. Изолированная проверка миграций заполненной БД, credential scan исходников/Git/артефактов, SHA-256 кандидатов и cleanup собственных Compose-ресурсов.

PASS допускается только при выполненном oracle. BLOCKED/NOT RUN остаются открытыми. GitHub private protection и успешное чтение с отказом записи тем же credential проверяются отдельно; их нельзя заменить GitLab, ошибочным токеном или имитацией. Установка принятого runtime остаётся отдельным этапом.


## Точки входа тестов

Имена ниже — точные Rust test selectors из исходников. Это каталог для запуска и сопоставления, а не результат исполнения. На Linux учитывать cfg(unix), integration feature и ignored атрибуты. Перед запуском одного сценария проверять `--list`; private worker/API helpers запускает только соответствующий acceptance test.

### Domain policy

[Исходник](../../backend/domain/src/repository_sync.rs). В crate `cicd-domain`:

| Назначение | Selector |
|---|---|
| Полная таблица направлений/refs/ancestry/protection | `repository_sync::tests::all_directions_preserve_refs_and_reject_rewrites` |
| Направления ancestry | `repository_sync::tests::directional_and_bidirectional_ancestry` |
| Tags/protection/missing refs | `repository_sync::tests::tags_protection_and_missing_refs` |
| Origin allowlist и credential-free URL | `repository_sync::tests::endpoints_are_explicitly_allowed_and_have_no_credentials` |
| Неоднозначные GitLab URL | `repository_sync::tests::gitlab_urls_reject_ambiguous_authorities_and_controls` |
| Канонический origin/base path | `repository_sync::tests::gitlab_uses_canonical_validated_origin_and_base_path` |
| Rust/Git authority не расходится | `repository_sync::tests::gitlab_url_cannot_change_authority_in_git` |

### PostgreSQL, HTTP и live acceptance

Все selectors следующего каталога относятся к `--features integration --test integration_db`. Для normal строк `ignored=no` обычный полный PG gate обязателен; `ignored=yes` требует явно подготовленного disposable fixture и отдельного запуска. Пропущенные ignored определения не включаются в число выполненных тестов. Каталог не ограничивает остальной общий PG suite.

| Исходник | ignored | Selector |
|---|---|---|
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::sync_protects_linked_project_defaults_without_explicit_entries` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | yes | `repository_sync::postgres_restart_preserves_unknown_intent_and_idempotency` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::sync_guard_serializes_concurrent_pr_merge_without_duplicate_commit` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::sync_http_defaults_cas_history_and_effective_permissions` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::additive_sync_migration_preserves_populated_repository_and_pipeline` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::sync_access_respects_memberships_and_machine_project_binding` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::incoming_event_ci_uses_exact_sha_and_shared_push_receipt` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | yes | `repository_sync::live_provider_worker_protection_automatic_and_lost_ack_recovery` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::durable_sync_cas_idempotency_coalescing_and_fencing` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::queued_disable_keeps_history_and_manual_mode_ignores_automatic_signals` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::namespace_archive_pauses_queued_and_blocks_commands_without_losing_history` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::machine_sync_settings_pass_full_http_middleware` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::automatic_signals_cover_all_enabled_directions_and_coalesce` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::sync_server_pages_large_lists_history_and_refs` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::busy_repository_guards_do_not_starve_other_queued_work` |
| [repository_sync.rs](../../backend/tests/support/repository_sync.rs) | no | `repository_sync::sync_put_is_allowed_by_configured_cors_preflight` |
| [repository_sync_available.rs](../../backend/tests/support/repository_sync_available.rs) | no | `repository_sync::available::available_hosted_catalog_pages_filter_exclusions_and_save_by_exact_uuid` |
| [repository_sync_list_scope.rs](../../backend/tests/support/repository_sync_list_scope.rs) | no | `repository_sync::list_scope::saved_connections_group_filter_preserves_uuid_namespace_and_combined_page_totals` |
| [repository_sync_api_contract.rs](../../backend/tests/support/repository_sync_api_contract.rs) | no | `repository_sync::api_contract::operation_keys_reject_other_authorized_actor_and_repository_over_http` |
| [repository_sync_api_contract.rs](../../backend/tests/support/repository_sync_api_contract.rs) | no | `repository_sync::api_contract::credential_and_malformed_command_rejections_preserve_saved_connection_over_http` |
| [repository_sync_api_contract.rs](../../backend/tests/support/repository_sync_api_contract.rs) | no | `repository_sync::api_contract::queued_running_and_reconciling_allow_only_disable_over_http` |
| [repository_sync_ci_pinned_config.rs](../../backend/tests/support/repository_sync_ci_pinned_config.rs) | no | `repository_sync::ci_pinned_config::incoming_event_compiles_configuration_and_jobs_from_exact_commit_not_later_head` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::api_process_crash_during_real_provider_check` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::empty_hosted_repository_initial_sync_and_head_preservation` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::actual_worker_crash_with_live_git_child_and_lost_ack` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::disable_after_accepted_push_preserves_actual_result` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::crash_before_push_retries_only_after_old_sha_observation` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::disable_before_push_never_sends_changes` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::crash_after_partial_progress_preserves_receipts_and_resumes_remaining_refs` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::all_directions_modes_tags_conflicts_and_actual_schedule` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::gitlab_git_only_scopes_and_revoked_credentials` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::viewer_login_reads_history_but_cannot_mutate_real_connection` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::github_read_only_credential_cannot_push` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::concurrent_external_change_is_never_overwritten` |
| [repository_sync_live.rs](../../backend/tests/support/repository_sync_live.rs) | yes | `repository_sync::live::actual_receive_pack_and_pr_merge_signal_automatic_sync` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::namespace_admission_connection_loss_during_gated_push_prevents_archived_write` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::heartbeat_connection_loss_cancels_git_and_reconciles_same_intent` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::namespace_admission_loss_after_accepted_push_reconciles_truthfully_without_repeating_effect` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::archive_wins_selected_run_admission_and_unarchive_resumes_same_run` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::archive_during_fetch_waits_for_real_namespace_lease` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::archive_before_push_waits_for_real_namespace_lease` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::crash_after_claim_before_plan_recovers_without_duplicate_effect` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::disable_during_fetch_stops_before_any_git_write` |
| [repository_sync_archive.rs](../../backend/tests/support/repository_sync_archive.rs) | yes | `repository_sync::live::archive::archive_after_unknown_effect_reconciles_without_new_writes_and_delivers_ci_after_unarchive` |
| [repository_sync_ci_delivery.rs](../../backend/tests/support/repository_sync_ci_delivery.rs) | yes | `repository_sync::live::ci_delivery::crash_after_pipeline_commit_replays_event_without_duplicate_pipeline_or_git` |
| [repository_sync_expired_credentials.rs](../../backend/tests/support/repository_sync_expired_credentials.rs) | yes | `repository_sync::live::expired_credentials::issuer_expired_credential_fails_safely_and_rotation_recovers` |
| [repository_sync_final_receipt.rs](../../backend/tests/support/repository_sync_final_receipt.rs) | yes | `repository_sync::live::final_receipt::crash_after_final_receipt_recovers_terminal_without_git_or_ci_replay` |
| [repository_sync_oracle_facets.rs](../../backend/tests/support/repository_sync_oracle_facets.rs) | yes | `repository_sync::live::oracle_facets::tree_object_tags_transfer_both_directions_without_commit_pipeline` |
| [repository_sync_oracle_facets.rs](../../backend/tests/support/repository_sync_oracle_facets.rs) | yes | `repository_sync::live::oracle_facets::manual_mode_real_receive_and_merge_stay_idle_beyond_interval_then_manual_sync_works` |
| [repository_sync_postgres_live_restart.rs](../../backend/tests/support/repository_sync_postgres_live_restart.rs) | yes | `repository_sync::live::postgres_live_restart::full_postgres_restart_with_live_git_before_effect_and_after_accepted_push` |
| [repository_sync_recovery_other.rs](../../backend/tests/support/repository_sync_recovery_other.rs) | yes | `repository_sync::live::recovery_other::unknown_push_then_third_sha_recovers_as_conflict_without_retry` |
| [repository_sync_slot_loss.rs](../../backend/tests/support/repository_sync_slot_loss.rs) | yes | `repository_sync::live::slot_loss::lost_slot_session_never_exceeds_live_git_capacity_and_waiting_repository_advances` |
| [repository_sync_timeout_cancel.rs](../../backend/tests/support/repository_sync_timeout_cancel.rs) | yes | `repository_sync::live::timeout_cancel::disable_after_native_git_timeout_reconciles_accepted_push_without_duplicate_effect` |
| [repository_sync_tls_chain.rs](../../backend/tests/support/repository_sync_tls_chain.rs) | yes | `repository_sync::live::tls_chain::api_and_os_worker_enforce_system_ca_and_hostname_for_real_git_exchange` |
| [repository_sync_workflow.rs](../../backend/tests/support/repository_sync_workflow.rs) | yes | `repository_sync::live::workflow::dispatch_only_workflow_transfers_both_directions_without_actions_runs` |

### Native Git и frontend

[Native adapter](../../backend/infra/src/sync_git.rs) содержит реальные local Git, guard inheritance/capacity, bounded output/pipes, безопасную очистку scratch и ENOSPC проверки. Exact opt-in TLS selector в crate `cicd-infra`: `sync_git::tls_tests::git_session_rejects_untrusted_and_wrong_hostname_but_accepts_system_ca`; требуется `FORGE_SYNC_PRIVATE_TLS_QA=1` в собственном disposable Linux fixture, поскольку тест временно добавляет свой CA в его system trust store. ENOSPC selector: `sync_git::tests::scratch_disk_full_is_safe_and_next_attempt_recovers`, opt-in `FORGE_SYNC_DISK_FULL_QA=1`.

| Frontend файл | Проверяемый уровень |
|---|---|
| [index.test.tsx](../../frontend/src/pages/repository-syncs/index.test.tsx) | Hosted picker, четыре направления, ручной/автоматический режим и exact save payload; CAS/unknown response; truthful states; callbacks поиска/фильтров/pagination, история и refs, namespace context |
| [repository-sync.test.ts](../../frontend/src/api/repository-sync.test.ts) | Настоящий typed client с контролируемым fetch: URL/method/body, operation key и AbortSignal; это transport test, не живой провайдер |
| [preview.test.tsx](../../frontend/src/previews/repository-sync/preview.test.tsx) | Только отдельный preview entrypoint; его fixtures не считаются production acceptance |

## Воспроизводимые gates результирующего кандидата

Команды выполняются на одном проверяемом candidate, после разрешения merge conflicts. Rust 1.88.0, Node 22.20.0, pnpm 10.28.1; Base SDK и UI checkout независимо сверяются с текущими pin-файлами по [BASE_INTEGRATION](../BASE_INTEGRATION.md). Зависимости locked/frozen. Функциональный release build выполняется без инструментального RUSTFLAGS. Инструментальные profile counters собираются отдельно и не заменяют обычные gates.

Контейнерные запуски только через Base ComposeHelper v2 в собственном временном Compose-проекте, с проверкой свободного места, owner/purpose и finally cleanup. Прямые standalone контейнеры, остановка runtime и удаление чужих volumes не являются частью проверки. API/worker тестируют собственные временные Git roots и БД. Установка кандидата в принятый runtime в эти команды не входит.

Для normal PG использовать job environment из [CI workflow](../../.github/workflows/ci.yml): отдельный `CICD_TEST_DATABASE_URL`, стабильные фиктивные `CICD_NAMESPACE__INSTANCE_ID`, `CICD_NAMESPACE__REGISTRY_INSTANCE_ID` и `CICD_NAMESPACE__PUBLIC_GIT_ORIGIN=https://forge-qa.invalid`. Origin нужен namespace-bound hosted readback даже без сетевого обращения к нему. CLI integration использует отдельную disposable БД, как в CI; не переключать её для остальных suites.

Backend из `backend`:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo test --locked --features integration --test integration_db -- --test-threads=1
cargo test --locked -p cicd-server --lib renewal_pg_ -- --ignored --test-threads=1
cargo test --locked -p cicd-server --lib namespace_devops_evidence_pg_ -- --ignored --test-threads=1
cargo test --locked -p cicd-server --lib owner_selection::tests:: -- --ignored --test-threads=1
cargo test --locked -p cicd-infra candidate_jobs_nested_names_stale_rows_duplicates_and_aliases_fail_closed -- --ignored --test-threads=1
cargo test --locked --features integration --test integration_db task_credential_successor_migration:: -- --ignored --test-threads=1
# Следующая команда использует отдельную CICD_TEST_DATABASE_URL для CLI.
cargo test --locked -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
cargo clippy --locked -p cicd-cli --all-targets --features integration -- -D warnings
cargo check --locked --workspace --all-targets
cargo build --locked --release --workspace
cargo run --locked --bin openapi-dump -- /tmp/forge-openapi.yaml
diff -u ../openapi/openapi.yaml /tmp/forge-openapi.yaml
```

Для отдельных CI opt-ins сверять ожидаемые количества: renewal PostgreSQL — два теста, namespace DevOps evidence — два, exact owner selection — один, artifact observer — один, populated successor migration — один. Каждый запуск должен завершиться с соответствующим числом `passed`, нулём `failed` и нулём `ignored`; успешный exit с нулём исполненных тестов не квалифицируется. Эти проверки общего CI не добавляют ID или подкейсы к исходным 50 ID и 108 сценариям синхронизации. CLI real-API target без integration feature исполняет ноль тестов и не квалифицируется этим запуском.

Frontend из `frontend`:

```sh
pnpm install --frozen-lockfile
pnpm audit --audit-level low
pnpm openapi:check
pnpm test -- --run
pnpm lint
pnpm exec tsc --noEmit
pnpm openapi:compat
pnpm build
python ../../services-base/scripts/check_ui_consumer.py
```

OpenAPI compatibility выполняется относительно свежего target branch. После build проверить effective themes штатным `pnpm theme:check` против собственного preview. Текущий lint включает shell contract из pinned Base. Generated schema и исходный lockfile после gates не должны измениться.

Репозиторные scripts/docs gates из корня:

```sh
python -B -m unittest scripts.tests.test_verify_docs scripts.tests.test_base_stand_backup scripts.tests.test_ci_contract_checks scripts.tests.test_postgres_rejection scripts.tests.test_backend_dockerfile scripts.tests.test_postgres_import_boundary
python -B scripts/verify_docs.py --all
python -B scripts/generate_sbom.py --check
```

Дополнительно выполнить native QA Compose ownership regressions и script syntax/compile из docs job текущего CI. SDK environment для native QA regression задаётся тем же pinned способом, что в workflow. Проверка secret scanner и точный credential audit охватывают итоговые source/объекты Git/новые артефакты; значения credentials в отчётах не публикуются. В переносимом merge report должны быть результаты и ссылки на доступные CI/PR artifacts с digest, а не ссылки на каталоги машины исполнителя.

## Условия live opt-ins

Каждый внешне применимый сценарий запускается отдельно для GitHub.com и разрешённого собственного GitLab. Fixture должен явно задать provider/origin/существующий repository и credential через защищённое окружение или stdin, как требует тест. Не помещать credential в URL, аргументы команды, Git config, tracked файлы или логи. GitHub read-only и issuer-expired tests требуют именно соответствующий credential: сначала доказанное положительное чтение, затем реальный отказ записи либо окончание issuer lifecycle; ошибочный token этого не заменяет.

Типовая команда для одного заранее подготовленного fixture:

```sh
cargo test --locked --features integration --test integration_db -- --list
# FORGE_SYNC_TEST — точный selector из каталога выше, без credentials.
cargo test --locked --features integration --test integration_db "$FORGE_SYNC_TEST" -- --ignored --exact --nocapture --test-threads=1
```

Physical PostgreSQL restart и native fault coordinators требуют отдельного собственного Compose-проекта, проверенной идентичности БД/процессов и marker protocol теста; запускать их как обычный cargo test без coordinator нельзя. Координатор и его digest прикладываются к qualification artifact. Production fault hooks не добавляются.

До внешнего прогона фиксируется baseline refs/SHA. После — сверяются actual refs/журнал/CI receipts; удаляются только собственные refs теста по точным именам и ожидаемым SHA, восстанавливается исходный baseline. Запрет автоматического удаления refs в продукте остаётся в силе. Изменение default branch, уничтожение исходных refs либо создание/удаление внешних репозиториев не является автоматической cleanup-операцией. Собственные provider tokens/runners, если нужны конкретному fixture, имеют отдельные provenance и cleanup readback.

Для UI приёмки нужны production entrypoint и настоящий API, без preview transport. Проверить URL/Back/Forward, клавиатуру, viewport из U-06 и применимые screenshot размеры PR template. Подмена API в component test должна быть названа; browser proof не выдаётся за установленный runtime, если использован отдельный candidate preview.

## Формат результата

Каждый обязательный подпункт получает provider, source tree/commit, exact selector или browser action, команду, outcome и проверяемый oracle. `1 passed` без реальной ожидаемой фазы не закрывает физический fault; exit 0 с нулём тестов не закрывает ничего. Failure до fixture precondition отделяется от product defect.

Публиковать line/function/region coverage отдельно от функциональной матрицы. Если Rust file-level summary включает inline cfg(test), это указывается до процентов; такой итог не называется production-only coverage. SIGKILL worker может не flush профили, а env_clear не наследует LLVM_PROFILE_FILE. Stable Rust branch coverage не обещается. Не достигать 100% тестами, зеркалящими реализацию; оставшиеся строки сопоставляются с содержательными oracle и ограничениями.
