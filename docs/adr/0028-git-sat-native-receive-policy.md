# ADR-0028: SAT Git authorization и native receive policy

Статус: source candidate, 2026-10-17; не installed/live proof.

## Контекст

API уже разрешает service account только внутри token project через stable
repository UUID. Git ошибочно требовал human membership. Одних Git scopes
недостаточно: direct receive обходил protected-branch gate существующего PR merge.
Workspace receipt не выдаёт Git credential и не авторизует push.

## Решение

Git SAT переиспользует существующий API repository/project predicate после
обычной проверки active credential и exact operation scope. Human ACL, public
read и legacy credential semantics сохраняются; новых scopes/DTO/migrations нет.

Перед SAT receive читается snapshot exact default/protected branch names всех
связанных CI-конфигураций. Git `check-ref-format` валидирует полученные refs.
Native `pre-receive` получает стандартный список old/new/ref от самого Git;
wire pkt-lines и pack не разбираются приложением. Любой запрещённый ref
отклоняет весь push, включая остальные refs, до их изменения.

Per-invocation private hook directory делегирует executable hooks исходного
effective `core.hooksPath`; guard предшествует custom pre-receive, которому
передаются исходные stdin/argv. Остальные hooks также сохраняют stdin/argv.
Git config и оригинальные hooks не модифицируются. Owner waiter получает
namespace admission lease до чтения branch policy и создания hook files,
удерживая её до завершения native Git независимо
от HTTP cancellation. Известный terminal outcome допускает scoped cleanup;
unknown writer outcome сохраняет files, без нового recovery controller.

## Границы и проверки

Прямое update/delete configured default/protected branch запрещено
для всех Smart HTTP writers; PR merge использует прежний gate. Первичная
публикация default branch в репозиторий без любых refs разрешена только
существующему admin или операторскому `CICD_GIT_TOKEN`, одним ref с нулевым
old OID. Git атомарно подтверждает отсутствие прежнего ref; следующий push
снова проходит обычный запрет. Service account, developer, read-only либо
foreign-project token не получают bootstrap. Custom hooks могут отказать
и при bootstrap. Без этого исключения связанный пустой репозиторий нельзя
инициализировать: прежний PR merge требует существующий target branch.
Непривязанный
bare repository не приобретает выдуманную CI branch policy. Политика фиксируется
на начале receive, не является новой транзакцией с последующими config edits.
Git scopes repository-wide, не task-branch credential или Tracker admission.
Guard directory находится внутри bare repository, не на потенциально noexec `/tmp`.

Добавлены шесть native Git unit regressions для repository-local placement,
protected/default refs, all-or-none
denial, task branch, custom hooks и ограниченного bootstrap. Два real-PG теста
входят в обычный integration gate: HTTP/Git проверяет SAT/PAT scope, bootstrap
и реальные refs; проверка advisory lock удерживает policy snapshot и доказывает,
что подготовка происходит под namespace write lease. Source и hook-script
checks не объявляют installed qualification.
