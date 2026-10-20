# Forge: участник lifecycle общего пространства v2

Статус: source candidate; установка, полный Space admission и serving cohort не
подтверждены. Текущая квалификация и незакрытые этапы находятся в отчёте workspace.
[Общий контракт](SPACE_ADMISSION_CONTRACT_V2.md),
[матрица доступа](UNIFIED_SPACE_SURFACES.md).

## Владелец и ресурсы

Admin владеет публичным SpaceRef, метаданными и lifecycle. Forge хранит свой
барьер и необязательную Git-группу. Создание Space не создаёт Git-группу,
репозиторий или CI-конфигурацию. Tracker ProjectRef не определяется через group ID
или внутренний CI projects.id. При первом применении existing legacy binding
сопоставляется только по registry_instance_id/namespace_id; UUID группы, ключ,
репозитории, aliases и история сохраняются. Имена не участвуют в сопоставлении.

Если legacy binding отсутствует, local_resource равен null. Подключение новой
Git-группы к уже созданному v2 Space требует отдельной явной owner-команды;
такой новый attach-сценарий ещё не реализован. Legacy v1 не используется как
неявный attach: после v2 он возвращает409. Текущий источник не меняет прежнюю
строгую схему OwnerCommand v1 и не переписывает её квитанции.

## API и полномочия

| Метод | Путь | Результат |
| --- | --- | --- |
| PUT | /api/v2/spaces/{registry}/{space}/owner-state | SpaceOwnerCommandV2 → SpaceOwnerReadbackV2 |
| GET | /api/v2/spaces/{registry}/{space}/owner-state | Текущая сохранённая квитанция; без изменения состояния |

Используются общие SDK DTO schema_version2. Обе операции требуют проверенный
центральный service_account PAT без browser session/делегированного grant,
регистрацию subject в CICD_NAMESPACE__OWNER_SUBJECTS и точный ci-cd:write.
Одного имени субъекта в allowlist, human admin hint или read scope недостаточно.
Проверяется исходный issuer/subject и срок токена; фиксированный monotonic deadline
не продлевается после SQL waits. В human users машина не материализуется.

CICD_NAMESPACE__REGISTRY_INSTANCE_ID и CICD_NAMESPACE__INSTANCE_ID сохраняют
прежних владельцев конфигурации. Path, registry и participant обязаны совпадать
со строгой командой. GET проверяет сохранённую команду/квитанцию и текущую
конфигурацию участника, а не возвращает произвольный JSON как успешный ACK.

## Операции, гонки и завершение

Исходный operation_id сохраняется вместе с полным payload, revision и owner
issuer/subject. Повтор того же payload возвращает его квитанцию. Подмена payload,
перенос ключа в другой Space, прежняя revision или смена slug не принимаются.
Завершённая старая квитанция доказывает ту историческую операцию; GET показывает
текущую. Pending archive повторяется под исходным ключом и обновляет свой ACK,
не создавая новую группу или задание. Следующая revision не обходит pending.

Порядок: original operation lock → общий Space lock → Git group exclusive
advisory lock → legacy binding FOR UPDATE → v2 catalog row. Старый owner v1
использует тот же Space lock; SQL fence защищает legacy binding при прямой
записи. Git write lease держит shared group lock до native process exit, поэтому
архив не подтверждается поверх ещё выполняющейся записи. SQL lifecycle guards
читают effective state под binding/catalog locks; после adoption состояние v2
приоритетно без изменения старого command JSON.

При архивировании catalog сначала переходит в archiving: новые pipeline/job/
lease starts и мутации ресурса запрещены. Уже работающим заданиям остаётся
terminal/control route. drained требует принятые completion receipts всех
начатых attempts. Lease expiry, terminal status или таймер не являются ACK.
При отсутствии подтверждений GET остаётся pending; PUT с исходным ключом
наблюдает настоящий drain и фиксирует archived. Принудительная остановка не
добавлена. Неподтверждённый старый архив нельзя восстановить через v2 adoption.

Restore меняет effective lifecycle на active, сохраняя данные и старую квитанцию.
Подтверждённый legacy archive может быть явно принят активной командой v2.
Admin переименовывает группу и в архиве; ключ/UUID/roles не переносятся. Обычный
редактор группы не является вторым владельцем этих метаданных. Архив проекта
Tracker и его M:N остаются отдельными ограничениями, не отменяемыми этим restore.

## Данные и пределы готовности

Аддитивная0096 создаёт forge_space_catalog/forge_space_owner_operations и
обновляет функции lifecycle guards. Исторические migrations/checksums/receipts
не редактируются. FK/identity guards сохраняют группы и историю; архивирование
и отвязка Space не удаляют проекты, репозитории, документы или artifacts.

Полный human/machine admission, новый явный Git-group attach, V3 frozen grants,
Git public/legacy/IO, raw/download/CI/runner routes, populated migration/access
comparison и совместимый rollback остаются отдельными обязательными gates.
Наличие этого owner endpoint не разрешает включить policy для части приложений
или использовать старые образы без admission после активации.
