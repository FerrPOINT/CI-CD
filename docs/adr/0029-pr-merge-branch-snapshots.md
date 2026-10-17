# ADR-0029: точные Git branch snapshots при PR merge

Статус: source candidate; установленный runtime не обновлён.

## Контекст

Короткое имя `main` может разрешиться в tag вместо `refs/heads/main`.
Прежние merge-tree/commit-tree тогда исключали актуальные данные target branch.
Без проверки старого target SHA update-ref также перезаписывал конкурирующий
merge. Проверка pipeline и последующее чтение движущегося source ref могли
относиться к разным commits.

## Решение

PR merge разрешает только branches через полные `refs/heads/...`. Короткое имя
и уже квалифицированное branch name приводятся к одной форме; tags, HEAD и
revision expressions не заменяют отсутствующую ветку. Source/target SHA
фиксируются до проверки pipeline. Policy и pipeline используют нормализованные
имена и точный source SHA; merge-tree и commit-tree получают зафиксированные OIDs.

Один native update-ref transaction проверяет source SHA и публикует target
с compare-and-swap по прежнему target SHA. Конфликт подготовки возвращает 409,
PR остаётся open. Успех требует terminal commit acknowledgement, а неизвестная
публикация — 503 с требованием readback без автоматического retry.

Существующий owner waiter сохраняет namespace admission до завершения принятой
операции при отключении HTTP caller. PR row lock сериализует merge/close/reopen;
каталог блокируется перед PR row, соблюдая порядок namespace → PR. Новых ролей,
credentials, scheduler, migrations или task protocols нет.

PR inline diff и ссылка на compare передают полные branch refs. Общий compare
сохраняет поддержку tags и произвольных Git revisions.

## Проверки и границы

Native unit tests проверяют одноимённые tags, сохранение обеих историй,
stale source/target и недопустимые refs. PostgreSQL/HTTP-router tests проверяют
pipeline gate, данные и ref parents, конкурентное изменение обеих веток,
HTTP disconnect и одновременное закрытие PR. Это не live-квалификация стенда.

Git refs и PostgreSQL не образуют общую транзакцию. Ошибка PG commit после
подтверждённой Git публикации требует readback исходной операции; автоматического
повтора merge нет. Durable reconciliation после process crash остаётся отдельной
работой и этим изменением не объявляется завершённой.
