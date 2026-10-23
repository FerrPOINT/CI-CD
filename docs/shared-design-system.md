# Подключение общей дизайн-системы

Продукт использует @pdlc/ui 0.3 и опубликованный документ Admin. Общие controls,
цвета, шрифты, плотность, скругления и popup принадлежат Base; запросы,
права, данные и callbacks сохраняются здесь. UI/SDK/skills pins независимы.

Канонический контракт: services-base/docs/platform/DESIGN_SYSTEM.md.
Компоненты искать через pdlc-ui search → show → example. Не создавайте
копии примитивов, собственный Toaster и глобальные определения Base tokens.
Существующие source-компоненты и preview получают общий UI; исторические
прототипы не являются установленным приложением. Установка выполняется отдельно.

Источник UI 0.3.0: `579749e19e9088be200c62e5563a3cb2db4d8c9f` в `.base-ui-revision`.
SDK и skills pins независимы; установка этим source-кандидатом не выполняется.

Vite plugins включает `createUiFontAssetsPlugin()` из `@pdlc/ui/build`: notices
встроенных fonts попадают в dist через общую реализацию Base.

## Сообщения, чтение и сверка операций — source-кандидат 2026-10-22

В 25 source-файлах Forge и действующем repository-sync preview ещё 103 композиции
используют Base: 16 LoadingState, семь EmptyState, 21 ErrorState, 58 Notice и
один ResourceStateView для динамического loading/error. Текст, условия, DTO,
права, обработчики и исходные operation keys остаются у Forge. Существующие
retry/readback-действия передаются в Notice.actions и не вызываются оформлением.
Частичная ошибка сохраняет последние подтверждённые данные с пояснением.
Семь простых role=status wrappers вокруг EmptyState сохраняют объявления;
это доступность, а не локальная копия оформления.

Последние scoped unit-результаты покрывают 196 cases; полный повторный прогон
не выполнялся. Селектор усечённого файла теперь проверяет текст внутри status,
учитывая отдельный output маршрута. TypeScript, scoped lint, choice/primitive и
shell guards прошли. Общий route gate открыт: текущего docs/ui-routes.json нет
в source/HEAD; историческая копия не подставляется как актуальная приёмка.

IAB source QA: 72 сочетания шести сцен, трёх тем и 320/375/1920/2560.
Неизвестный результат сохранения оставляет черновик; явная сверка сохранения
и запуска сохраняет исходные key/payload. На каждую из двух синтетических
операций — один первый запрос и один явный readback. Повтор чтения аудита
сохраняет подтверждённые строки при ошибке. Overflow нет, мобильные однострочные
controls не меньше 44 px. Product backend, credentials и installed runtime
не использованы. Итоговые pins, архив и интеграционный gate остаются открытыми.

## Общие секции, метрики и карточки — 2026-10-22

Ещё 25 мест в 14 файлах используют Base: 13 SectionHeader, четыре рамки
Card/CardContent (включая форму подключения), два CardTitle, две отчётные Metric,
одна композиция шести dashboard Metric, два EmptyState preview и один Notice.
Убраны локальные рамки заголовков с действиями и оформление карточек репозиториев.
Карточка каталога сохраняет целую ссылку; namespace, идентичные короткие имена,
отсутствующее хранилище и archived state продолжают обрабатываться Forge.
Отчётные показатели сохраняют форматирование и доступные имена регионов.

114 scoped unit cases прошли; после уточнения string-контракта EmptyState повторены
восемь preview cases. TypeScript, scoped lint, choice и shell guards прошли.
AST-сравнение подтверждает non-render logic, события, формы, refs, URL и ARIA;
идентификаторы секций передаются через headingId, показатели — именованные Metric.
Новый browser gate композиций остаётся для итогового этапа; текущий route inventory
ещё отсутствует. CapabilityCallout сохраняет предметный h2 внутри общего Notice.

## Все шапки страниц — 2026-10-22

25 h1 в 22 файлах, включая действующие dev-preview, используют PageHeader.
Иконки и переход назад передаются в leading, контекст проекта/подключения — в
context, прежние действия — в actions. Хлебные крошки сохраняют порядок перед
шапкой. Формы, disabled/pending, фильтры, namespace и URLs не менялись; это
подтверждено AST-сравнением. Единственный оставшийся raw heading Forge — название
предметного сообщения CapabilityCallout внутри Notice Base.

Scoped unit: последние результаты 209 cases в 27 файлах прошли. Старое ожидание
сырой ошибки offline воспроизведено до переноса шапки; четыре pipeline cases
проверяют общее сообщение чтения, отличие от пустоты и однократный retry.
TypeScript, scoped lint, choice и shell ownership прошли. Финальные browser,
package/pins и полный route inventory остаются открытыми gates.
## Общая карточка показателя проекта — 2026-10-22

Ссылки из сводки workspace используют Card/CardContent с inline Metric Base.
Данные и формат URL, включая project_scope и namespace, остаются у Forge.
Локальные рамки, padding и focus удалены. 21 router case, TypeScript, scoped
lint, choice и shell guards прошли. Новая продуктовая браузерная проверка
остаётся для интеграционного кандидата.

## Метки изменений и исполнения — 2026-10-22

Четыре локальных метки используют Badge Base: статус сравниваемого файла,
prerelease, runner и pipeline/deployment. Forge сохраняет соответствие статусов
семантическим ролям: added/success, modified/warning, deleted/danger;
online/success, paused/warning, offline/muted. Ожидающие исполнения имеют warning,
успешные success, неуспешные danger, остальные muted. Текст статуса остаётся
видимым; декоративная точка runner теперь принадлежит Base.

67 scoped unit cases прошли, включая три состояния runner без побочных записей
и три статуса изменённых файлов. TypeScript, scoped lint, choice и shell guards
прошли. AST-сверка сохраняет события, формы и URL; менялись только согласованные
presentation adapters. Полный route inventory и новый product browser gate
остаются отдельными незакрытыми проверками.

## Формы и браузер репозитория — 2026-10-22

Ещё девять локальных рамок заменены общими композициями: пять форм PR,
репозитория и CI-конфигураций через Card.asChild/form + CardContent; связанная
задача PR через Card; два представления file/tree через CardHeader/CardContent;
пустое исполнение через EmptyState с прежними message, description и action.
Нативные формы, FormData, journal/readback, permissions, filters, URLs и handlers
сохранены. Общие padding, рамка и радиус больше не объявляются продуктом.

59 scoped unit cases в шести файлах, TypeScript, scoped lint, choice и shell
guards прошли. AST-сверка подтверждает non-render logic и поведенческие
атрибуты. Отсутствие кандидатов в текущем JSX-фильтре простых рамок не закрывает
более широкий CSS-аудит, актуальный route inventory и product browser gate.

## Общие опасные действия — 2026-10-22

Восемь кнопок удаления environments, releases, runners, schedules, secrets,
webhooks и целевой строки используют Button destructive-outline/size=icon.
Локальные цвета, border, focus, disabled paint и размеры удалены; владельцем
подтверждения, pending, прав и mutation остаётся Forge. Flex shrink оставлен
только как часть предметной компоновки.

83 scoped cases десяти файлов прошли; TypeScript, scoped lint и primitive gate
прошли. Общий danger hover исправлен в Base. Source-проверка использует private
qualification config; frozen package/peer graph, route inventory и продуктовый
browser gate остаются открытыми.

## Общие ссылки-действия — 2026-10-22

Два действия перехода к pipelines в отчётах, включая успешную пустоту используют Button asChild variant=link.
Цвет, hover, focus и область нажатия задаёт Base; ссылки остаются одним native
anchor, адреса и namespace/project scope сохраняются у продукта. Отступ mt-3
в пустом отчёте остаётся предметной компоновкой, не оформлением контрола.

Scoped unit, TypeScript, lint и primitive guard прошли; AST-сверка подтверждает
прежние данные, маршруты, условия и события без подмены результата запроса.
Реальная продуктовая browser-приёмка и точный UI package/pin остаются открытыми.
Ссылки внутри текста не превращаются в полноразмерные кнопки; их общий контракт
проверяется отдельно.

## Общие ссылки исполнения и окружения — 2026-10-22

Четыре текстовые ссылки исполнения и один проверенный внешний URL окружения
используют TextLink. Три самостоятельных действия списка исполнения используют
Button asChild variant=link. Валидатор URL, rel/target, namespace, фильтры и
callbacks остаются в Forge; AST и 14 scoped unit cases подтверждают контракт.

TypeScript, scoped lint и primitive guard прошли. Общие компоненты проверены
в IAB на собственных синтетических сценах; фактическая продуктовая browser-
приёмка и поставка точного UI archive/pin остаются частью итогового gate.

Группы периода, параметров синхронизации и её preview используют Fieldset Base.
Нативные disabled, legend, labels и события сохраняются. Доступность записи,
режимы, фильтры, URL и запросы остаются у Forge; общий компонент не получает
Git-состояние или credentials. При переходе удалено локальное оформление legend,
предметные grid/spacing остаются. UI pin и installed runtime этим не обновляются.
