# Локальный Compose runner

Локальные jobs и transfer helpers принадлежат настоящему Compose-проекту
`sdlc-build-job-<attempt UUID>`. Backend содержит Docker CLI и Linux Compose
5.5.1 с закреплёнными checksums. Обновление daemon для plugin не требуется.

Регистрация workspace задаёт TLS endpoint, CA/client certificates, ожидаемый
daemon ID, immutable source volume, Base SHA и имя workspace. Переменные:
`DOCKER_HOST`, `DOCKER_TLS_VERIFY=1`, `DOCKER_CERT_PATH`,
`CICD_RUNNER_DOCKER_DAEMON_ID`, `CICD_RUNNER_SHARED_SOURCES_VOLUME`,
`CICD_RUNNER_BASE_REVISION`, `CICD_RUNNER_WORKSPACE_PROJECT`.
Поддерживаются зарегистрированные installations `sdlc1`, `sdlc2`, `pdlc1`.
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

После прерывания backend сверяет journal, daemon, labels, execution attempt
и состояние БД перед новой диспетчеризацией. Оборванный attempt становится
failed с неизвестным результатом исполнения; ресурсы очищаются через его
Compose manifest. Автоматического replay нет. Повтор требует явного retry API.
Чужие labels или неправильный daemon блокируют cleanup и dispatch.

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
