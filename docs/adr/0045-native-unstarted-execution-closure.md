# ADR-0043: Отмена Kubernetes Pod, не создавшего контейнер

Статус: кандидат; установка и live qualification отдельно.

Kubelet после ошибки image pull и удаления Pod может вернуть Failed /
ContainerStatusUnknown без containerID, imageID и finishedAt. Это не является
полным доказательством остановки. Прежний runner сохраняет custody и блокирует
следующее исполнение; такое поведение выявлено при переносе установки.

Выбран минимальный дополнительный источник фактов: существующий привилегированный
Base guard читает CRI containers, sandboxes, containerd tasks и Pod cgroup.
Неизвестный task, сохранённый execution container, живой sandbox или populated
cgroup запрещают отрицательное наблюдение. Guard публикует только ограниченный
набор свежих наблюдений в отдельный operator-owned ConfigMap. Он не завершает
lease, не удаляет Pod и не запускает команду.

Forge использует наблюдение только при явном Kubernetes opt-in
CICD_KUBERNETES_NATIVE_CLOSURES=true и отдельном read-only mount
/run/forge-native-closures/closures.json, принадлежащем root. Контекст связывает
installation hash, CA, namespace/PVC UID, Pod UID/resourceVersion и node.
Наблюдение старше 90 секунд либо от чужого контекста отвергается. Только точная
Failed/deleting/ContainerStatusUnknown форма без известных признаков запуска
может получить failure sentinel 137 / NativeVerifiedNotStarted. Время выхода
не выдумывается. Обычные terminal observations, отсутствие Pod, неизвестный
запущенный контейнер и успешное выполнение сохраняют прежние проверки.

Существующий runner завершает исходный lease штатным protocol, сохраняя runner ID,
attempt, generation, token и source checks. ACK появляется только от backend.
Неизвестный результат не разрешает повтор команды. Дополнительная таблица,
сервис, scheduler, публичный API или изменение Docker/Compose defaults не нужны.

Рассмотрено принятие одного Failed API-статуса: отклонено, поскольку оно теряет
native stop proof. Ручная подмена finishedAt/ACK также запрещена. Отдельный
сервис наблюдения не нужен: факт уже принадлежит штатному host guard.

Проверки: точное свежее наблюдение; stale/foreign UID; изменённый RV; известный
containerID/startedAt; активная cgroup; неизвестный task; обычный terminal path;
реальный cancel и следующий запуск. Docker/Compose без opt-in не читает
operator mount и не требует Kubernetes.
