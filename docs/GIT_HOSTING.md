# Git Hosting — Forge CI/CD

Source candidate: [write admission PR](adr/0037-pull-request-write-admission.md)
сохраняет прежние DTO/URL, добавляет opaque request permit, atomic PR number
transaction и original proof до native publication. GET связанных задач PR
использует общий repository read. [Durable merge recovery](adr/0038-durable-pull-merge-recovery.md) использует
сохранённый intent и точные refs; повтор не запускает native publication.
Добавлена миграция 0098; runtime и исторические migrations не изменены.
Полный совместимый cohort остаётся отдельным gate.

Native deadline cleanup сначала даёт Git250ms на SIGTERM, затем завершает
остаток owned process group через SIGKILL. Локальные ref locks убирает сам Git;
при forced termination нельзя удалять неизвестные locks или считать rollback
доказанным. Неизвестная publication требует exact ref readback.

Source candidate: [общий read admission Git-браузера и PR](adr/0036-repository-browse-read-admission.md)
использует decoded owner identity и original proof. Read adapters и PR write
покрываются scoped native HTTP/Git/SQL fixture; exact inputs и результаты находятся
в отчёте задачи. Это source candidate, не установленная политика и не готовность
всего serving cohort.

Opt-in source-only [own-task candidate Git](OWN_TASK_CANDIDATE_GIT.md) проверяет
confirmed source/current Developer и exact OID CAS через native hooks. Только
этот child-вариант подавляет post-receive auto-pipeline; обычный human/SAT поток
ниже не меняется. Qualification/installation не заявлены.
CLI `repository push-candidate`, включая `--readback`, использует только private
Git child/context до общего API-клиента, без global-token fallback. Durable
single-attempt marker и правило unknown outcome → readback-only не изменены.
Новый собственный checkout получает nonhuman local author defaults только для
отсутствующих effective `user.name`/`user.email`; global/explicit identity и
существующие checkout/rework сохраняются. Это не новый principal или grant.
Task-only hooks используют `/api/v1/internal/task-git-check`, не legacy
`/api/v1/internal/git-push`. Последний сохраняет свой object contract и отвергает
`task_candidate` до обычной pipeline обработки.

Удаление repository с immutable SDLC workspace operation history возвращает409;
owner FK RESTRICT сохраняет DB identity, bare repository и workspace. Проверка
выполняется атомарным DELETE; filesystem cleanup начинается только после успешного
DB удаления. Удаление project с такой history также возвращает409. Это retention
boundary, не разрешение удалять evidence или обходить original operation key.

## 1. Назначение

Forge CI/CD включает минимальный self-hosted Git-сервер. Он хранит bare-репозитории, реализует Git Smart HTTP, проверяет read/write доступ к private/write операциям и создаёт CI/CD-пайплайн после каждого push.

Это не замена GitLab или Gitea: в текущем объёме есть минимальные pull requests/compare, но нет issues, web editor, LFS API, отдельных Git-пользователей или прав на уровне репозитория. Цель MVP — замкнуть локальный цикл `git push -> pipeline` без внешнего Git-провайдера.

## 2. Реализованный поток

```text
Developer
  |
  | git clone / git fetch / git push
  v
Dashboard :22802 (nginx)
  |
  | /git/* proxy
  v
Backend :22801
  |
  | git upload-pack / git receive-pack --stateless-rpc
  v
bare repository volume: cicd_git_repos
  |
  | post-receive hook
  v
POST /api/v1/internal/git-push
  |
  v
Project lookup -> queued pipeline (build -> test -> deploy)
```

## 3. API управления репозиториями

| Method | Path | Назначение |
|---|---|---|
| `GET` | `/api/v1/repositories` | Список bare-репозиториев |
| `POST` | `/api/v1/repositories` | Создать bare-репозиторий |
| `DELETE` | `/api/v1/repositories/{name}` | Удалить bare-репозиторий |

### Создание

```bash
curl -fsS -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"platform-core"}'
```

Имя содержит только ASCII letters/digits, `-`, `_`, `.`; запрещены leading `.`, `..` и пробелы. Суффикс `.git` при передаче автоматически убирается.
Новый bare-репозиторий создаётся с символическим `HEAD` на `refs/heads/main`,
поэтому первый push в `main` сразу делает обычный clone работоспособным без
ручной настройки default branch на сервере.

### Git URL

```bash
git clone http://127.0.0.1:22802/git/platform-core.git
```

В local Compose это идёт через frontend nginx proxy (`:22802`), который проксирует `/git/*` в backend. Прямой backend Git endpoint также доступен на `http://127.0.0.1:22801/git/<repo>.git` для диагностики. В production заменить хост и порт на публичный URL reverse proxy.

## 4. Smart HTTP

Backend использует системный Git для стандартного stateless RPC:

| Git operation | HTTP route | Git service |
|---|---|---|
| discovery | `GET /git/{repo}/info/refs?service=...` | `git upload-pack` / `git receive-pack --advertise-refs` |
| clone/fetch | `POST /git/{repo}/git-upload-pack` | `git upload-pack --stateless-rpc` |
| push | `POST /git/{repo}/git-receive-pack` | `git receive-pack --stateless-rpc` |

Nginx frontend контейнера проксирует `/git/` в backend. Внутренний backend также доступен на `:22801` для диагностики.

## 5. Связь с проектом и auto-trigger

При создании проекта укажите URL локального Git-репозитория:

```bash
curl -fsS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{
    "name":"platform-core",
    "repository_url":"http://127.0.0.1:22802/git/platform-core.git",
    "default_branch":"main"
  }'
```

Каждый созданный репозиторий получает executable `hooks/post-receive`. Hook отправляет storage identity, pushed ref, `old_rev` и `new_rev` на internal endpoint. Backend разрешает stable repository UUID по однозначному alias и читает явную push policy: одна CI-конфигурация либо «Отключён». Совпадение URL tail и выбор первой конфигурации не используются. `project_id` по-прежнему обозначает CI-конфигурацию.

Повтор того же hook event с тем же `repository/ref_name/new_rev` сохраняет исходное push-решение в `repository_push_operations`, включая отключённый запуск. Изменение policy не перенаправляет повтор на другую конфигурацию. `pipeline_triggers` возвращает существующий `pipeline_id` для исходного запуска. Удаление ref (`new_rev` из нулей) не запускает pipeline.

Если проект не связан с репозиторием, push остаётся успешным, но pipeline не создаётся.

## 6. Pull requests и refs

PR merge разрешает именно branch refs, фиксирует source/target SHA до проверки
pipeline и публикует результат через native source verification + target CAS.
Одинаково названный tag не меняет merge или PR diff. При изменении любой ветки
возвращается 409 без закрытия PR; неизвестная публикация требует readback, без
автоматического retry. Merge/close/reopen сериализованы по строке PR, принятую
операцию завершает owner waiter при отключении HTTP caller.
Границы PG/Git атомарности: [ADR-0029](adr/0029-pr-merge-branch-snapshots.md).

Основной каталог pull requests читается страницами через
`GET /api/v1/repos/{repo}/pulls/page`. Сервер применяет `limit`, `offset`, фильтр
`status` и регистронезависимый `search` до выборки; ответ возвращает `items` и
`total`. Карточка PR использует отдельный
`GET /api/v1/repos/{repo}/pulls/{number}` и поэтому не зависит от текущей
страницы каталога. Legacy `GET /api/v1/repos/{repo}/pulls` сохраняет прежний
массив целиком только для обратной совместимости; frontend его не использует.

`GET /api/v1/repos/{repo}/refs` сохраняет тип каждого ref: `branch`, `tag` или
`other`. В форме создания PR подсказки source и target ограничены `branch`;
ручной ввод остаётся доступен, если каталог refs временно недоступен.

## 7. Конфигурация

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_GIT_ROOT` | `/var/lib/forge/git` | Корень bare-репозиториев в backend container |
| `CICD_GIT_TOKEN` | empty | Legacy shared token Smart HTTP. Empty отключает shared bypass; без `CICD_AUTH_SECRET` это допустимо только для local development |
| `CICD_GIT_INTERNAL_TOKEN` | empty | Токен для `post-receive -> internal endpoint`; empty допустим только для isolated local development |

Данные репозиториев находятся в named volume `cicd_git_repos`, независимом от PostgreSQL volume. Удаление проекта не удаляет репозиторий; удаление репозитория через API удаляет и строку БД, и bare directory.

## 8. Аутентификация

Ответ Smart HTTP `401` содержит `WWW-Authenticate: Basic realm="Forge Git"`
для discovery и RPC. Обычный Git-клиент после challenge получает credential
из своего helper; передавать токен в clone URL не требуется.

Git Smart HTTP различает read и write:

- `git-upload-pack` для repository с `visibility = public` доступен без credential.
- Private `git-upload-pack` и любой `git-receive-pack` требуют credential.
- Legacy `CICD_GIT_TOKEN` остаётся operator bypass для совместимости.
- При непустом `CICD_AUTH_SECRET` можно передать JWT/PAT пользователя: `viewer+` читает связанный project repository, `developer+` может push, `admin` имеет bypass. PAT дополнительно должен иметь `git:read` для fetch/clone или `git:write` для push и, если задан `project_id`, совпадать со связанным проектом. Для центрального PAT исходное подтверждение Auth и scopes сохраняются отдельно от локальных claims: отсутствие локального `api_tokens.id` не означает browser session. Срок повторно проверяется после асинхронной ACL-проверки; scope не заменяет подтверждённую роль.
  Private proof сохраняет исходный срок также на native advertisement/RPC,
  owner lock wait и body handoff; private, no-store запрещает кешировать
  защищённый Git response. Native receive проверяет fixed deadline после custom
  prepared reference-transaction hook. По истечении native owner завершает
  собственную process group и ждёт Git, сохраняя write lease до cleanup.
  Неизвестный результат требует exact ref readback без автоматического retry.
  Linux boot-clock bridge округляет оставшееся окно вниз; последняя неполная
  секунда закрывается. Уже отправленные байты отозвать нельзя. Подробности и
  оставшиеся public/legacy/task admission gates — в [матрице](UNIFIED_SPACE_SURFACES.md).
- Project-bound `forge_sat_` использует тот же repository UUID → CI project
  predicate, что API, без фиктивного human membership. Нужны отдельные
  `git:read`/`git:write`; `api:read`/`api:write` не дают Git-доступ.
  Disabled/revoked/expired SAT отклоняет существующий identity resolver.

Связь hosted repository → project проверяется через `repository_catalog.id`,
`storage_name` и `projects.repository_id`, не через URL tail.

Smart HTTP receive использует native `pre-receive` до изменения refs: прямые
update/delete default branch или точного имени из `protected_branches`
любой связанной CI-конфигурации запрещают весь push. Изменение этих branches
проходит через существующий PR merge gate. Только admin либо существующий
операторский `CICD_GIT_TOKEN` может впервые создать один default ref в
репозитории без любых refs, с нулевым old OID. Service account, developer,
read-only и foreign-project credentials не получают bootstrap. Mixed push,
создание другого protected ref, повторный update и delete запрещены и оператору.
Custom hooks могут отказать первичной публикации. Task branches продолжают
работать по прежним ACL/scopes.
Новый task-bound credential не выдаётся, `git:write` остаётся repository-wide.

Guard создаётся внутри bare repository на его Git volume, не в `/tmp`.
Временный private `core.hooksPath` делегирует executable custom hooks исходного
effective hook directory с прежними argv/stdin. Repo hooks и Git config не
перезаписываются. Namespace write lease получается до чтения policy и
создания hook files и сохраняется до завершения native Git. При известном
завершении Git удаляется только собственный
guard directory; при неизвестном writer outcome он сохраняется. Подробности:
[ADR-0028](adr/0028-git-sat-native-receive-policy.md).

Если используется legacy `CICD_GIT_TOKEN`, Git HTTP принимает один из вариантов:

```bash
# Basic auth: username произвольный, password = token или JWT/PAT
git clone http://any-user:<TOKEN>@host/git/platform-core.git

# Или для raw HTTP запросов
curl -H "x-git-token: <TOKEN>" ...
```

В trusted local development без `CICD_AUTH_SECRET` и без `CICD_GIT_TOKEN` проверка отключена. Нельзя запускать shared/public Git endpoint в таком режиме.

`CICD_GIT_INTERNAL_TOKEN` ведёт себя так же строго по границе окружения: пустое значение отключает проверку только для trusted-local hook traffic, shared deployment обязан задать уникальный токен. Устаревшее значение `forge-internal-dev-token` отклоняется при старте backend.

Opt-in source общего пространства (migration0097, default OFF) требует central
human identity и Admin policy на discovery/RPC/aliases до public/legacy fallback.
Unbound repository закрыт; read admission не выдаёт write роль. Нужна точная
v2 revision mapping и active состояние для записи. Старый task V1 не наследует
human admission. Включать partial source нельзя; [cohort и оставшиеся gates](UNIFIED_SPACE_SURFACES.md#opt-in-human-git-space-admission-source-candidate).
Существующая установка и поведение выше описаны отдельно от этого source slice.

## 9. Ограничения MVP и следующий этап

- Каталог Git groups/repository UUID и project-bound SAT уже существуют. Общее Space admission ещё не установлено: public/legacy Git и прочие поверхности требуют отдельной квалификации полного совместимого cohort. Current read/write RBAC использует memberships и PAT scopes через `projects.repository_id`, а не URL tail.
- Нет Git LFS HTTP endpoints несмотря на наличие `git-lfs` в образе.
- Нет SSH transport: только Smart HTTP.
- Hook делает best-effort запрос; он не блокирует push при временном сбое CI/CD API.
- Embedded runner клонирует project repository в workspace перед выполнением job; external runner protocol MVP уже отдаёт `workspace.checkoutUrl`, а `forge-runner` использует его для checkout. Scoped repository credential class и production checkout policy ещё не реализованы.

Следующая фаза общего пространства: [Space admission v2](SPACE_ADMISSION_CONTRACT_V2.md) и [матрица поверхностей](UNIFIED_SPACE_SURFACES.md). Git LFS/SSH transport и прочие будущие возможности не входят в эту реализацию. Фактические serving listeners и ingress проверяются отдельно перед активацией.

## 10. Проверка

```bash
# Создать репозиторий и убедиться, что discovery работает
curl -fsS -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"smoke-repo"}'
git ls-remote http://127.0.0.1:22802/git/smoke-repo.git

# Удалить после проверки
curl -fsS -X DELETE http://127.0.0.1:22801/api/v1/repositories/smoke-repo
```

## References

- [API](API.md)
- [Workflow](FUNCTIONAL_ARCHITECTURE.md)
- [Webhooks](contracts/EVENT_CONTRACT.md)
- [Security](SECURITY.md)
- [Storage](contracts/DATA_LIFECYCLE.md)


## Git в Namespace

Git Group, стабильный repository UUID, aliases и archive admission описаны в [Namespace](NAMESPACE.md) и [ADR 0023](adr/0023-namespace-repository-identity.md). Старые bare paths и clone URLs сохраняются; отсутствующее storage требует восстановления исходных данных.

## Внешняя синхронизация

Hosted-репозиторий может иметь одно HTTPS-подключение к GitHub.com или
разрешённому оператором GitLab. Направления: отключено, внешний → Forge,
Forge → внешний, обе стороны. Запуск вручную или автоматически; ручная
кнопка доступна в обоих способах. Новый connection отключён и запускается
вручную. Общий список — «Синхронизации», контекстный переход — из репозитория.

Передаются все ветки и теги с прежними именами. Удаления не распространяются.
При расходящейся истории или изменении существующего тега фиксируется
конфликт. Защищённая ветка Forge требует обычного PR; автоматического PR нет.
Git LFS, SSH, issues/PR/releases, provider webhooks и создание remote отсутствуют.

Для GitLab используйте read_repository / write_repository по направлению.
Для GitHub fine-grained PAT выберите нужный репозиторий, Contents: Read для
входящего обмена, Read and write для исходящего; если передаются workflow-файлы,
учтите отдельное разрешение Workflows. Classic PAT для private repo требует repo,
для workflow-файлов — workflow. Токен вводится только в защищённое поле формы.
Проверка доступа читает refs и делает dry-run для исходящего направления;
окончательное разрешение push проверяет провайдер при реальной записи.

История показывает снимок подключения, источник запуска и каждый ref.
«Уточняется результат» означает, что Forge сверяет фактический SHA после
неподтверждённого эффекта. Архив приостанавливает исполнение, не удаляя историю.
Остановку подтверждают после завершения процесса и сверки.

Default branch каждого связанного проекта защищена и при пустом списке
protected branches. Sync учитывает те же ветки, что обычный Git receive;
входящие изменения этой ветки требуют обычного PR.

См. [ADR-0031](adr/0031-external-repository-synchronization.md).

Права токенов: [GitHub](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens), [GitLab](https://docs.gitlab.com/security/tokens/access_token_scopes/).

Теги на blob/tree передаются, но не запускают CI: у них нет commit для checkout.
Причина ci_non_commit_tag сохраняется в результате ref; остальные CI-события
доставляются независимо, с повтором временных ошибок.

Автоматические подключения всех трёх направлений получают сигнал после
локального push/merge и сверяются каждые 5 минут. Сигналы объединяются в один
следующий запуск. Занятый Git guard одного репозитория не занимает место
исполнителя для других репозиториев; постоянный guard по-прежнему берётся
после namespace admission и наследуется дочерним Git-процессом.

Лимит одновременных sync-исполнений дополнительно удерживается через persistent
`flock`-файлы `.forge-sync-capacity/<slot>.lock` на общем Git root. Git child
наследует слот вместе с mutation guard, включая первый процесс создания scratch.
Потеря PostgreSQL slot-сессии и смерть worker не освобождают ёмкость, пока жив
его Git helper. Файлы слотов нельзя удалять во время работы. Все workers
используют один Git root и одинаковый concurrency; снижение лимита требует
сначала остановить приём новых запусков и дождаться завершения старых процессов.

Worker проверяет живость именно транзакции namespace admission перед Git,
во время исполнения и после него. Отдельный heartbeat журнала не подтверждает
сохранность этого соединения. Ошибка или таймаут проверки отменяет Git-обмен
и оставляет запуск в `reconciling` с причиной `namespace_admission_lost`.
Guard удерживается до остановки дочерних процессов и записи результата.
Следующее исполнение заново проверяет admission и фактические SHA; уже принятый
провайдером push не становится ложным «Остановлено». Проверка соединения имеет
ограниченное время обнаружения и не образует транзакцию с удалённым push.

Проверка доступа хранит временные объекты отдельно от запусков, в
`.forge-sync-checks`. Следующая проверка того же repository UUID убирает
оставшийся после прерывания scratch только после захвата его Git guard.

Генератор `post-receive` сохраняет допустимую Git кавычку в имени ref через
JSON-экранирование. Иначе обычный push такой ветки теряет событие CI и ближайший
сигнал синхронизации. Проверка исполняет настоящий shell hook и разбирает его
payload; Git/PR acceptance использует ветки с кавычками.

Таймаут и отмена Git-обмена охватывают также чтение stdout/stderr после
завершения основного процесса. Если HTTPS helper удерживает каналы открытыми,
отмена завершает принадлежащую запуску группу процессов; результат остаётся
неподтверждённым до сверки refs. Завершение родительского Git само по себе
не подтверждает получение результата от всех его дочерних процессов.

Полный набор регрессий и условия внешних проверок описаны в
[плане приёмки синхронизации](plans/2026-10-18-repository-sync-complete-test-suite.md).
Актуальные результаты кандидата и ограничения приведены в
[отчёте подготовки к merge](reports/2026-10-18-repository-sync-merge-readiness.md).


## Две роли и сохранённый product PAT: source candidate

При обязательной application policy cicd_ проверяется по действующей записи,
центральному UUID владельца и fresh Auth owner proof. Локальный admin flag не
назначает центрального admin. Прежние git:read/write, API scopes и project pin
сохраняются. При включённом space admission видимость разрешается только тому
же центральному владельцу; приложение и пространства проверяются независимо.

Native Git/ref preparation получает минимум исходных deadlines владельца,
credential и пространства. Durable merge сохраняет original clock после
перехода к detached owner. Истечение вызывает ограниченный process cleanup и
unknown outcome с exact ref readback; старый callback не публикуется повторно.
Это описывает feature-исходники: installed cohort и real job acceptance остаются
отдельными gates, а закрытые task V1/machine adapters ещё требуют реализации.
