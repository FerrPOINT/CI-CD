# ADR-0038: durable recovery результата PR merge

Статус: принято для source candidate.
Дата: 21 октября 2026 года.
Владелец: Forge. Уточняет recovery-часть [ADR-0037](0037-pull-request-write-admission.md).

## Контекст

Git refs и PostgreSQL не образуют общую транзакцию. Detached owner сохраняет
операцию при HTTP disconnect, но crash, timeout после публикации или ошибка
записи статуса PR могут оставить опубликованный merge с `status=open`.
Без заранее сохранённого candidate OID повтор способен создать новый merge
вместо подтверждения уже выполненного результата.

Нужен owner-local intent, а не новый общий orchestrator или replay native effect.
PR status, ancestry и текущий «похожий» commit не определяют результат попытки.

## Решение

Аддитивная миграция 0098 создаёт `pull_request_merge_attempts`.
Одна неотклонённая попытка на PR содержит неизменяемые repository/PR ID,
точные source/target refs, исходные OID и подготовленный merge OID.

1. Namespace lease остаётся у detached owner. Merge получает существующий
   repository MutationGuard **до** SQL row locks и держит его через весь протокол.
   Другой merge не занимает SQL connection в ожидании этого guard.
2. Под прежними role/lifecycle и PR locks проверяются branch snapshots,
   protected-branch pipeline gate и исходный fixed-clock proof. Готовый commit
   сохраняется в intent; транзакция коммитится **до** ref publication.
3. Owner открывает новую транзакцию, снова берёт role/lifecycle и PR locks и
   проверяет **тот же** proof. Это не refresh срока. Новая попытка после ожидания
   не может обойти архивирование, отзыв роли или истечение допуска.
4. Native update-ref сохраняет прежние source/target CAS и custom hooks.
   Определённый отказ до publication коммитит `rejected`; новая независимая
   попытка разрешена. Неопределённый исход оставляет `prepared`.
5. После подтверждённой publication статус PR и `confirmed` сохраняются одной
   SQL-транзакцией. Любая ошибка или истечение proof до commit оставляет durable
   intent для последующего readback, без rollback refs по предположению.

Lock-файлы, custom hooks, cleanup protocol и public DTO/routes сохраняются.
После intent commit close/reopen видят `prepared` и возвращают 409. Они не могут
замаскировать unknown publication сменой статуса PR.

## Повторный запрос

Обычный `action=merge` сначала ищет существующий intent. Требуются текущие
полномочия, scopes и lifecycle admission; прежний истёкший proof не возрождается.

- `prepared` и PR open: source ref обязан равняться сохранённому source OID,
  target ref — сохранённому merge OID. Тогда меняется только SQL metadata;
  merge-tree, commit-tree, update-ref и publication hooks не вызываются.
- `confirmed` и согласованная merged metadata: возвращается сохранённый результат,
  без нового Git effect. Позднейшее нормальное продвижение ветки не является
  повторной публикацией этого завершённого merge.
- Любое расхождение, включая descendant target, изменённый source, отсутствующую
  ветку или исходный target OID, не даёт автоматического нового merge. Результат
  остаётся unknown и требует отдельного операторского разбора.
- Репозиторий/PR/branch mapping должны совпадать с intent. Несогласованная metadata
  не исправляется эвристикой.

Параллельные повторы сериализуются MutationGuard и PR row lock: первый подтверждает
SQL, второй читает `confirmed`. Candidate OID не переснимается и не пересоздаётся.

Отсутствующая или непригодная Git ref не доказывает отсутствия прежнего эффекта.
Неизменный старый target после timeout тоже не доказывает rollback: intent не
сбрасывается автоматически. Это намеренный fail-closed предел восстановления.

## Схема и поставка

Миграция аддитивна; старые checksums и applied receipts сохраняются. Журнал
содержит только server-owned ID/refs/OID, без credentials. FK RESTRICT удерживает
историю попыток при удалении PR/repository.

Новый backend требует миграцию 0098 даже при policy OFF. Аддитивность схемы не
означает совместимость смешанных backend: старый образ игнорирует intent и может
повторить публикацию. После создания intents rollback допустим только на
совместимый cohort с сохранением журнала. Полный migration/rollback rehearsal
и проверка обслуживающих маршрутов остаются gate до dev activation.

Отдельная операторская процедура для неподтверждённых/изменившихся refs этим
срезом не вводится. Она не должна удалять intent или обновлять refs по догадке.
Эксплуатационная проверка полноценного process restart остаётся отдельным gate;
наличие durable SQL row само по себе не является live restart evidence.

## Проверка

Адресный native HTTP/PostgreSQL/Git target покрывает реальный post-publication
timeout, SQL failure после изменения refs, конкурентные повторы, drift source/
target, отзыв scopes/роли, запрет close для pending и отсутствие повторного
publication hook. Соседний свежий PR по-прежнему достигает custom deny hook,
после определённого отказа допускает отдельную подтверждённую попытку.

Результаты конкретного source, ресурсы и ограничения находятся в отчёте задачи;
наличие этого ADR не означает установленную политику или готовый полный cohort.

## Последствия и альтернативы

Появляется одна дополнительная durable SQL стадия перед ref effect. Это стоимость
восстановления межсистемной операции; отдельный worker/таймер не требуется.
Автоматический повтор native merge, определение результата по ancestry и сброс
unknown intent по таймеру отвергнуты: они не доказывают исход прежней публикации.
