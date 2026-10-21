# ADR-0037: общий допуск и исходный срок записи PR

Статус: opt-in source candidate, qualification в работе. Установленная политика
не менялась; неполный serving cohort активировать нельзя.

## Причина и выбранная граница

Read admission не является правом изменения. Middleware-only проверка не
покрывает ожидание SQL, блокировку merge и native custom hook. Повторное получение
решения после ожидания могло бы создать новое окно вместо исходного срока.

Сохраняем существующие owner boundaries: Admin/SDK подтверждают видимость
SpaceRef; продуктовые роли и scopes подтверждают изменение; Forge namespace
lease блокирует archive; MutationGuard сериализует Git; native reference
transaction сравнивает точные source/target SHA. Новые сервисы, issuer, header
authority, зависимости, tables и варианты strict-v1/v2 не добавляются.

## Request permit и транзакции

Для существующих create/action маршрутов legacy repo/catalog UUID
и привязки задачи к PR middleware разрешает framework-decoded owner identity и получает bounded proof
с active Space и совпадающей owner revision. Оно сохраняет scopes, текущую
продуктовую роль и credential project binding. Private fields opaque
RepositoryWrite создаются только проверенным middleware, не DTO или caller header.

HTTP adapter передаёт permit в typed use case, включая detached native merge
owner. При активной политике direct/internal caller без permit также закрыт.
OFF сохраняет прежний режим без вызова Admin. Никакого implicit Space/Project нет.

После namespace lease SQL transaction блокирует подтверждённого local user
FOR SHARE, проверяет enabled/актуальную роль и исходный proof, затем подтверждает
current SpaceRef/revision/state под блокировкой. Роль и lifecycle сохраняются до
окончания effect. Начальный proof не обновляется после ожиданий.

Создание PR и выделение его номера теперь выполняются одной транзакцией:
истечение proof во время ожидания counter не оставляет ни PR, ни новый номер.
Close/reopen сохраняют прежние transitions и проверяют proof после row lock и
после SQL, перед commit. Это соответствует существующей owner transaction
границе; отдельный background writer/retry не создаётся.

## Привязка задачи и повтор запроса

POST task links получает тот же private RepositoryWrite до чтения stored result.
При active policy replay подтверждает текущие scopes, роль и lifecycle; наличие
строки не возвращает доступ после отзыва. После namespace lease роль/catalog
блокируются до окончания операции. Новая связь записывается транзакционно;
исходный proof проверяется после Tracker readback и перед/после INSERT, до commit.
SQL wait не создаёт новое окно. Исходный TaskRef/commit у replay сохраняется;
Tracker повторно не вызывается. OFF сохраняет исторический original-key readback,
включая уже сохранённую связь архивного owner; новые записи требуют прежний lease.

Fixture дополняется replay scope/role/hidden deny, encoded ID, expiry при реальной
блокировке таблицы и сохранением source commit. Эти подготовленные cases требуют
native запуска; cargo check подтверждает только типы. Новый INSERT после задержки
Tracker/SQL, машинная делегация и строгий V3 остаются отдельной приёмкой.

## Native merge и неизвестный результат

Merge-tree/commit-tree и update-ref используют существующий fixed-clock native
Git owner. Он завершает только свою process group и ждёт cleanup до освобождения
namespace lease/MutationGuard. HTTP disconnect не отменяет detached owner.

При истечении исходного срока owned process group сначала получает SIGTERM:
Git выполняет собственный cleanup ref locks. Ожидание ограничено250ms, затем
оставшаяся группа получает SIGKILL, включая hook, игнорирующий TERM. Это время
очистки не продлевает допуск и не разрешает публикацию после proof deadline.
Принудительное завершение не доказывает Git rollback: unknown outcome по-прежнему
требует exact readback. Lock-файлы не удаляются по предположению об их владельце.

Перед native ref preparation private hook выполняет исходный custom/task hook,
затем проверяет оставшееся время исходного proof через прежний conservative
BOOTTIME/wall cap. Custom hook bytes не переписываются. Одна Git transaction
проверяет обе refs; подмена source/target между snapshot и publication запрещена.

Timeout не доказывает отсутствия publication. После возможной публикации,
ошибки записи merged metadata или потери подтверждения возвращается явный
unknown outcome: требуется exact ref/PR readback, автоматического retry нет.
Защищённый ответ private/no-store и не переживает original proof.

На момент этого среза PR API не содержал durable intent для unknown merge.
Эта часть заменена [ADR-0038](0038-durable-pull-merge-recovery.md): сохранённый
intent разрешает metadata recovery по точным refs и сериализует повтор.
Выбор по ancestry, повторная native publication и признание одного PR status
доказательством Git rollback по-прежнему запрещены.

## Покрытие и приёмка

Create/action проверяются по canonical ID и legacy/encoded aliases. GET связанных
задач PR включён в ту же repository read boundary; POST task links — в write
boundary. Machine evidence и task/project V3 authority остаются отдельными поверхностями. Прежние roles, task scopes, immutable mapping,
исторические owner receipts и данные сохраняются.

Native fixture использует реальные HTTP/Git/SQL: scope/role/foreign deny,
counter+PR rollback после original expiry в SQL, close/reopen, custom prepared
hook deadline и cleanup, exact успешная publication, timeout после committed
hook с реальным ref readback и без повторного запуска, task-link read deny.
Fixture создаёт настоящий source commit скрытого репозитория перед проверкой
stored replay. Native hook разрешается через git rev-parse --git-path hooks,
включая configured core.hooksPath. Проверяются отсутствие own ref locks после
штатного TERM cleanup, достижение запрещающего hook и следующий успешный merge;
конфликт из-за stale lock не считается доказательством работы custom hook.

Подготовленные cases и компиляция не являются native PASS, live Admin/IAB
приёмкой, общей миграцией или full gate. Результаты и версии — в task report.
