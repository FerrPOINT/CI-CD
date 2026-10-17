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
Git config и оригинальные hooks не модифицируются. Owner waiter удерживает
namespace admission lease и hook files до завершения native Git независимо
от HTTP cancellation. Известный terminal outcome допускает scoped cleanup;
unknown writer outcome сохраняет files, без нового recovery controller.

## Границы и проверки

Прямое создание/update/delete configured default/protected branch запрещено
для project-bound SAT. Human/legacy receive сохраняет исходные ACL, initial push
и custom hooks без нового guard; тип principal вычисляет только сервер после
identity/project/scope checks. PR merge использует прежний gate. Непривязанный
bare repository не приобретает выдуманную CI branch policy. Политика фиксируется
на начале receive, не является новой транзакцией с последующими config edits.
Git scopes repository-wide, не task-branch credential или Tracker admission.
Guard directory находится внутри bare repository, не на потенциально noexec `/tmp`.

Добавлены четыре native Git unit regressions для repository-local placement,
protected/default refs, all-or-none denial, task branch и custom hooks;
один ignored real-PG/HTTP/Git aggregate для
correct/foreign/unbound SAT, read-only/API-only/disabled credentials и ref outcomes.
Aggregate также проверяет human initial/default push и сохранение viewer deny.
Heavy Rust/PG execution выполняется отдельно после Main admission; source и
hook-script checks не объявляют installed qualification.
