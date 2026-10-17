# Локальный Compose runner

Локальные jobs и transfer helpers принадлежат настоящему Compose-проекту
`sdlc-build-job-<attempt UUID>`. Backend содержит Docker CLI и Linux Compose
5.5.1 с закреплёнными checksums. Обновление daemon для plugin не требуется.

Регистрация workspace задаёт TLS endpoint, CA/client certificates, ожидаемый
daemon ID, immutable source volume, Base SHA и имя workspace. Переменные:
`DOCKER_HOST`, `DOCKER_TLS_VERIFY=1`, `DOCKER_CERT_PATH`,
`CICD_RUNNER_DOCKER_DAEMON_ID`, `CICD_RUNNER_SHARED_SOURCES_VOLUME`,
`CICD_RUNNER_BASE_REVISION`, `CICD_RUNNER_WORKSPACE_PROJECT`.
Поддерживаются зарегистрированные installations `sdlc1`, `sdlc2`, `pdlc1`, `pdlc2`, `pdlc3`.
Workspace и полный lowercase Base SHA должны точно совпадать с labels sources;
старый volume без provenance не присваивается автоматически новой установке.
Несовпадение daemon ID, отсутствие plugin или source provenance блокирует job.
Fallback на другой Docker context запрещён.

Каждый attempt получает собственные workspace/cache volumes и сеть. Sources
подключаются readonly. Transfer выполняется Compose-сервисом того же attempt.
Сохраняются seccomp, resource limits, exit code, timeout, cancellation, logs
и artifacts. Job не получает runner TLS, Docker endpoint или control journal.

Журнал и manifest находятся в `.compose-control` существующего volume,
смонтированного в backend как `/workspaces`. Каталог имеет mode 0700, файлы —
0600. Manifest может содержать job secrets; его нельзя публиковать как artifact.
После успешного `compose down --remove-orphans --volumes` environment удаляется
из сохранённого manifest. Удаляются только volumes данного attempt; immutable
sources объявлены external и сохраняются.

Выход Docker CLI не доказывает остановку job: перед terminal completion требуется
свежий Engine readback остановленного собственного контейнера. Timeout явно
останавливает только контейнер текущего attempt и повторяет readback. Cleanup
проверяет ownership контейнеров, volumes и сетей, затем свежий terminal receipt
своего attempt/job. Неизвестный completion не разрешает удаление.

Drop только сохраняет `cleanup-required`; фоновой команды down нет. Неподтверждённый
выход сохраняет PID либо reservation в обёртке runner. Живой владелец с неизвестной
остановкой блокирует новую диспетчеризацию до reconciliation; автоматический
replay отсутствует. После рестарта recovery подтверждает остановку до освобождения
lease и удаления файлов. Чужие owner/project/manifest/resource names блокируют
действия. Неизвестное или отозванное acknowledgement сохраняет данные.

Retry API также читает существующий journal каждого embedded attempt своей job.
Сброшенная при рестарте память и умерший owner не разрешают новую attempt/queue:
до `cleaned` с `stopped=true` API возвращает `409`. Проверяются attempt/job,
daemon, control volume и manifest path; повреждение, symlink и неполный существующий
каталог блокируют retry. Чтение ограничено 64 KiB для journal и 1 MiB для manifest.
Отсутствующий каталог legacy/shell либо prelaunch failure не создаётся заново.
API не выполняет stop/cleanup: подтверждение по-прежнему публикует штатный recovery.

После прерывания backend сверяет journal, daemon, labels, execution attempt
и состояние БД перед новой диспетчеризацией. Оборванный attempt становится
failed с неизвестным результатом исполнения; ресурсы очищаются через его
Compose manifest. Автоматического replay нет. Повтор требует явного retry API.
Чужие labels, foreign volume users/network members или неправильный daemon
блокируют stop/cleanup и dispatch до первой операции с ресурсами.

Pulse размещается только в приватном Git/project CI/CD `sdlc1`. При импорте
сначала передаются проверенные commits и tags, затем регистрируется project:
`main`, владелец — администратор `sdlc1`, `max_running_jobs=1`. Pipeline проверяет
Rust 1.88, frontend, OpenAPI и browser tests с mock API/Auth и собственным
webServer. Sources Base экспортируются из точного commit штатным Base CLI.

Локальные shell helpers требуют явные `SDLC_WORKSPACE_DIR`,
`SDLC_DOCKER_CONTEXT` и `SDLC_TASK`. Backup wrapper дополнительно требует
`SDLC_PROJECT`, сохранённый `SDLC_SIGNING_KEY` и защищённый output archive;
использует полный workspace profile и согласованную остановку писателей.
Retention или удаление старых данных wrapper не выполняет.

Backend gate и Trivy scanner используют отдельную временную сеть для загрузки
Rust-компонентов, зависимостей и базы уязвимостей. Сеть принадлежит QA-проекту
и удаляется при cleanup; к постоянным сетям workspace helpers не подключаются.

Лимит проекта проверяется до резервирования lease в SERIALIZABLE transaction.
Встроенный и внешний runner учитывают активные leases, включая подготовку.
Serialization conflict оставляет job queued без запуска; лимит Pulse равен одному.

## Native QA Compose transport

`scripts/native_qa_compose.py` используется только для временной native QA.
Host harness передаёт `docker_context`, ожидаемый `daemon_id` и проверенный
`sdk_sha256` в `NativeComposeBridge` и `recover_session` явно. `load_sdk`
принимает тот же `sdk_sha256`. Значений по умолчанию для подключения нет;
hash берётся из квалифицированного SDK, а не вычисляется как разрешение
доверять произвольному текущему файлу. Эти поля не принимаются через RPC.

Все команды используют выбранный context. Изменение daemon за тем же context
блокирует выполнение. Журнал v2 сохраняет endpoint, daemon ID и hash SDK;
recovery требует точного совпадения с явно выбранным назначением, прежних
ownership/manifest guards, мёртвого владельца и возраста от 24 часов.
Старый журнал нельзя перепривязать к новому SDK или чужому daemon.

Если native backend и daemon используют разные файловые пространства, host
harness может явно передать `daemon_bind_volume` в bridge и recovery. Это
словарь с точными `project`, `volume`, `cleanup_id`, `purpose` и `mountpoint`
родительского disposable QA volume. Project имеет вид `sdlc-qa-forge-<hex>`,
volume — `<project>_delivery-qa`; mountpoint берётся из actual `volume inspect`
того же daemon. RPC не принимает и не меняет эту привязку.

Перед использованием adapter сверяет actual Name, Mountpoint и полные owner
labels с host binding. Только нормализованные абсолютные POSIX paths внутри
этого volume передаются в существующий SDK `daemon_bind_sources`; mounts
остаются read-only с `create_host_path=false`. Остальные пути проходят прежние
локальные проверки. Замена volume owner/mountpoint блокирует новый запуск или
переход до изменения manifest. Binding сохраняется в journal v2; recovery
требует той же явной привязки и повторного readback. Cleanup удаляет дочерние
ресурсы, сохраняя родительский volume.

Привязка проверяет принадлежность volume, а не содержимое удалённого файла.
Штатный OCI snapshot transport по-прежнему отвечает за отсутствие symlinks,
владельца, permissions, размер и hash файлов. Этот QA adapter не заменяет его
проверки и не предоставляет произвольный daemon path через native RPC.

Pure regression suite запускается отдельным процессом:
`python3 -B scripts/tests/test_native_qa_compose.py`, с явно заданными
`SDLC_MAINTENANCE_BASE` и `SDLC_MAINTENANCE_SDK_SHA256` в окружении host harness.
Она использует настоящий SDK и модель Docker, не запускает контейнеры и
не подтверждает native build/deployment или полную SDLC-приёмку.
