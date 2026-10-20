# ADR-0036: общий допуск чтения Git-браузера и PR

Статус: opt-in source candidate. Native qualification этого среза ещё не выполнена:
preflight остановил запуск до компилятора из-за недостаточного запаса памяти.
Установленный runtime и activation state не изменялись.

## Причина

Каталог репозиториев и native Git уже сохраняют исходный proof, но REST чтение
дерева, blob и PR использовало только прежние роли/membership. Дополнительно
ACL разрешал alias через каталог, а Git-браузер принимал URL-имя за storage name.
Идентичность ресурса расходилась между проверкой доступа и чтением.

## Решение

Существующий auth middleware выбирает явно перечисленные read adapters по
framework MatchedPath и получает decoded RawPathParams, как конечный handler.
Это не raw URI split, выбор первого проекта или доверие заголовку пользователя.

Для legacy repo alias и catalog UUID сначала читается только storage identity
из параметризованного owner mapping. Kind/readiness не выдаются до допуска.
При active policy существующий bounded SDK подтверждает точный SpaceRef через
repository → group → owner catalog, используя private verified central identity
и исходный deadline. Машинный субъект не получает human admission.

Проверка выполняется до чтения Git/PR, counts и pagination. Common read заменяет
прежнее read membership; api/ci-cd scopes, global route role и явный project binding
credential сохраняются. Это не разрешение изменения или администрирования.
OFF не вызывает Admin admission. Архивирование не отменяет допустимое чтение.

После handler и перед каждым body frame снова проверяется тот же original proof;
ответ private/no-store. Ожидание SQL не продлевает срок. Existing catalog adapter
использует тот же response guard без изменения DTO.

Git-браузер разрешает decoded alias в storage через существующий каталог и
валидирует именно storage name перед filesystem path. PR merge использует тот
же resolver, но разрешение merge и его effect boundary остаются отдельным этапом.

## Поверхности и ограничения

Read adapters: legacy refs/tree/blob/tags/commits/compare, PR list/page/detail,
catalog PR list/detail. GET обрабатывается явно; HEAD не получает отдельного
разрешения и по-прежнему подчиняется общей route policy.

Create/action/merge PR, task links, releases, CI, sync, artifact/download и
machine/V3 adapters этим срезом не квалифицированы. Частичную политику включать
нельзя. DTO, URL, зависимости, historical migrations и image/SDK pins не меняются.

Native fixture дополнен реальными HTTP/Git/SQL cases: selected/all/empty,
random owner IDs и aliases, percent-encoded repo/UUID/separator, hidden public и
unbound, hidden readiness, точные blob bytes, PR total/offset, scope/machine deny,
archived read и expiry при реальной PR SQL-блокировке. Это подготовленные cases,
а не полученный PASS. Owner admission в fixture замокан; живой Admin, полный
suite, serving cohort и IAB требуют отдельных gates.
