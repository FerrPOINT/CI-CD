# ADR-0043: управляемые окружения разработки

Дата: 2026-10-31. Статус: реализация; API, квалификация и установка ещё не завершены.

## Модель и владельцы

Workspace — постоянный экземпляр среды проекта, а не каталог попытки CI и не
TaskWorkspace SDLC. Профиль принадлежит продукту; Base валидирует его и владеет
общими operator-примитивами; Forge хранит экземпляры, сессии, операции и связи
с существующими pipeline. Fleet и Developer-агент этим изменением не добавляются.

Экземпляр ссылается одновременно на настоящие Tracker instance/project UUID,
репозиторий Forge, delivery configuration и зарегистрированный execution environment.
Существующее поле projects.id Forge не подменяет идентификатор проекта Tracker.
Одна активная изменяющая сессия и одна незавершённая операция защищены уникальными
индексами и блокировкой строки workspace. Законченная операция и исходный запрос
не переписываются. Сессия хранит настоящие UUID/key задачи и выбранные исходники.

## Профили и execution targets

UI выбирает опубликованный профиль и зарегистрированный target. Кластерные
credentials, каталоги и команды не поступают от клиента. Новое окружение получает
свои namespace, адрес, storage, runtime/signing credentials и пустые dev-данные.
Operator заранее регистрирует изолированную площадку и её разрешения/квоты;
создание продуктового окружения из профиля выполняется через Forge.
Creator и обычный lifecycle operator имеют разные разрешения. Выделение namespace
не превращает обычный start/stop в cluster-admin API. Для пилота каждая публикация
target является отдельной площадкой; уникальная связь не допускает повторно занять
тот же адрес/namespace. Несколько площадок могут использовать один профиль.

Существующий R1 подключается наблюдением с сохранением UID ресурсов, Secret
versions и release identity. Его локальный checkout имеет исторический путь
r1-upgrade-v0114; это имя не обозначает текущую версию приложения.

## Подготовка и проверки

CLI на машине разработчика подготавливает явные committed sources, сохраняя
HEAD, index, dirty/untracked файлы существующего checkout. В Forge передаются
ограниченный отчёт машины и выбранные commit, без доступа сервера к её каталогам.
Сервер проверяет binding репозиториев, наличие commit и hash профиля.

Инструменты имеют location=local/forge. preparationChecks профиля перечисляют
опубликованные проверки инструментов, независимые от запущенного приложения.
R1 проверяет backend и frontend в отдельных immutable images. Frontend image
связан с hashes package.json/package-lock.json; зависимости не ставятся при Start.
Готовность исходников/инструментов, runtime, рабочая сессия и результаты тестов
остаются разными признаками. Успешные процессы не означают прохождение тестов.

Именованные проверки используют существующие parsing, immutable pipeline plan,
human owner, runner admission, queue и results. Private adapter передаёт только
опубликованный recipe; API не принимает произвольный shell или pipeline YAML.
План помечается development_workspace и содержит exact source/profile/recipe.
Idempotency key исходной операции связывает потерянный ответ с тем же pipeline.

## Runtime и unknown outcome

Изменяющая API-команда сохраняет operation UUID, ожидаемую версию и hash запроса.
Worker заново проверяет исходного владельца, native права и общий Space admission.
Отзыв прав не заменяет владельца техническим пользователем или другим admin.

Base operator использует тот же UUID и приватный immutable journal. Kernel lock
передаётся процессу до запуска Python и его native командам. Потеря HTTP/DB связи
и timeout не освобождают workspace. Повтор читает исходный результат. Реконцилиация
не запускает заново исходное действие.

Kubernetes scale использует UID/resourceVersion/replica preconditions. После
закрытия исходного command owner recovery ставит новый resourceVersion barrier
и наблюдает результат, сохраняя desired replicas. Для Compose неизвестный daemon
effect нельзя подтвердить снимком running/stopped: требуется исходный native
receipt. При его отсутствии операция остаётся unknown и блокирует новые effects.
Сценарий потерянного ответа при сохранённом terminal receipt восстанавливается
чтением исходного результата. Owner tooling для отсутствующего receipt является
обязательной частью квалификации Compose, а не основанием ослабить guard.

desired_runtime хранится отдельно от фактического runtime. Stop и delivery
используют один допуск: остановленное окружение не может включиться от Helm apply.
Проверяются его pipeline и deployments, а не все сборки того же проекта. Нельзя
выключить занятую сессию; Start допускается свободному окружению или владельцу
его текущей сессии. Unknown сохраняет блокировку до подтверждённого recovery.

## Поставка и незавершённая работа

Аддитивная 0113 следует за общей SQLx convergence 0112 из ADR-0042. Новые identity
references включаются в существующий deletion inventory и write guards.
Откат сохраняет записи и требует совместимого lifecycle/delivery admission;
старый binary без этого guard нельзя незаметно вернуть после регистрации сред.

До приёмки обязательны worker/creator, полный CLI/UI, PG/concurrency/auth gates,
реальные Kubernetes/Compose сценарии, подготовка R1, его recipes и оба экземпляра,
backup/recovery, поставка в обычный PDLC и свежая IAB/runtime/data проверка.
Текущие исходники и Draft PR не являются свидетельством выполнения этих gates.
