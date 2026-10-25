# Общие подписи полей Base

## Текущий кандидат: кодовые поля Base — 25 октября 2026

UI producer `7432f931cddcba29f0c6bc91ca2cf6663baa771f`, reference `9154aea77e9265a2abca532e909b5c3836678814123aeee51d3fc36703b7b4f5`, версия 0.3.2.
В этом продукте 3 подтверждённых кодовых Input/Textarea получают общий шрифт без изменения TSX, данных и обработчиков.
Base исправляет приоритет font-mono у Input/InputWithAction/Textarea одним CSS-
правилом. Обычный шрифт и минимум 40/44 px сохранены; Textarea сохраняет собственную
высоту, autoSize учитывает переносы нового шрифта в пределах min/max-height.

Свежие проверки волны: 17 field, 61 CSS/primitive, 86 reference и 41 Workflow
SSR cases; types/examples, настоящий архив/offline CLI и fingerprints PASS.
IAB: 105 положительных React/SSR/standalone/density контекстов плюс ввод, focus,
action/reset и рост/сжатие autoSize; 18 исходных контекстов подтвердили дефект.
Прежние продуктовые types/unit/lint переиспользованы по неизменным JS/TS inputs,
они не названы новым прогоном или доказательством CSS. Шесть cached frozen refresh
проверили fingerprint и React singleton. AI file-URI обновлён явно; installs не
изменили подготовленные locks. SDK/skills pins сохранены.
Source QA не заменяет live-приёмку продукта. Installed-header Playwright regression
подключён к существующему manual CI gate; локально этот runner не запускался.
Полный milestone gate, hosted CI и установка остаются отдельно. Общий аудит
продолжается; детали — план Base shared-ui-consolidation-20261021.

## Предыдущий кандидат: смысловые подписи и общие действия — 25 октября 2026

UI producer `bbfec663f0cd9e3c92c75bb0e1f367b17f682f76`, reference `dea80c36822dcfc94dd3ca81970bac0c0c4868d94718708fac180b2cc510b4c0`, версия 0.3.2.
SDK/skills pins сохранены; шесть frozen offline refresh сохранили locks и React singleton.

Два действия repository sync/preview используют variant=link без локального paint. Настоящее действие запуска явно имеет type=button; callbacks и URL-state сохранены. 64 теста двух областей, lint, types и guard прошли.

Base проверен отдельно: 17 React, 35 primitive/SSR и 85 reference cases.
Вся scoped-волна: 13 source + 27 consumer stages, 343 unique cases, архив/offline CLI,
SSR/Auth freshness. IAB: 288 публичных UI-контекстов, native events/reset, namespace
href и keyboard/focus; source QA закрыт без сетевых записей. Это не live-приёмка
продуктовых страниц. Полный milestone gate, hosted CI и installation остаются отдельно.
Подробности — план Base shared-ui-consolidation-20261021; private evidence:
control-presentation-20261025/. Общий аудит продолжается.

## Предыдущий кандидат: описание и отступы карточек — 25 октября 2026

UI producer `4f9140a66d99f32ca656e5008022d25a960d0e31`, reference `1c1f0d864adfd8a23dcf4f656be59cc746c9a915308396c29387ad0f267134d2`, версия0.3.2.
SDK/skills pins независимы и сохранены.

Обновлён только UI pin; продуктовый код и контракты не менялись. TypeScript и UI guard прошли; новые браузерные проверки страниц Forge этой волной не заявляются.

Общий контракт — CardDescription/default/muted и card_description SSR.
Отступы CardHeader/CardContent принадлежат density tokens; локальные padding
обнаруживает существующий guard. Полная AST-сверка восьми файлов с негативным
контролем допускает только заявленные presentation-замены, без изменения логики.
Шесть frozen offline refresh сохранили lockfiles и React singleton.

Общие доказательства:10 source +24 consumer scoped stages;279 unique cases
(8 Base React,34 primitive/SSR,84 reference,112 consumer,41 Workflow SSR).
Настоящий архив:802 файла,1120797 bytes; offline CLI, SSR/Auth freshness PASS.
IAB72 computed-style контекста и24 standalone контекста:три темы,три плотности,
standard/large и320/375/1920/2560. Прежнее оформление и отступы сохраняются;
keyboard link/focus/Back/reload проверены. Это source QA, не live-приёмка продукта.
QA закрыт, hashes стабильны, сетевых записей нет. Hosted CI, built Storybook,
real zoom200 и installed/authenticated acceptance остаются отдельными gates.
Private evidence:card-presentation-20261025/; общий план Base продолжается.

## Предыдущий кандидат: общие страницы отказа и отсутствующего ресурса — 25 октября 2026

UI producer `c2be7939e8b7622a29e1a30ff1c8b6d5ae731a59`, reference `d74c7cccb8a70fb2ca78348071037ed3af045fb5e0cbe0109327de3e4f52ffb3`, версия0.3.2.
SDK/skills pins независимы и не менялись.

ForbiddenPage использует ResourceStateView вместо своей геометрии, цвета и типографики. В продукте остаются локализованные title/message, glyph и NamespaceLink возврата. Сохраняются h1 и реальная ссылка с Namespace. QueryState по-прежнему выбирает этот предметный 403-адаптер; политика доступа и другие read states не менялись.

Base владеет необязательными title/headingLevel/icon/back для терминальных
ResourceStateView. Defaults остаются h2 с прежними подписями; самостоятельная
страница может использовать h1 и типографику PageHeader. Иконка декоративна,
геометрия/цвет/отступы принадлежат общим tokens. Новый standalone пример и story
Terminal добавлены без изменения прежних IDs, schema2 и API v1.

Кандидат прошёл27 scoped stages и адресный повтор затронутых Fleet checks после
уточнения h1. Итоговые наборы:113 React cases,83 reference и41 Workflow SSR
(237 unique, без сложения повторов), types/lint/format, шесть consumer guards,
examples, настоящий архив/offline CLI и SSR/Auth freshness. Frozen offline refresh
сохранил consumer locks и React singleton; у AI Hub заменён только URI UI-снимка.

IAB:72 контекста Base/реальной Forge page,9 проверок плотности и48 контекстов
публичных состояний с точными Fleet route props; standard/large и три темы,
320/375/1920/2560 без overflow. Проверены отступы,40/44px, Namespace keyboard
return, Back/reload и однократное действие standalone примера. Fleet admission
проверена отдельно на реальных route elements с тестовым источником permissions.
Это source QA без credentials и установленного backend. Обе QA-сессии закрыты,
hashes стабильны, сетевых записей нет. Built Storybook, real zoom200, HTTP runtime
parity, hosted CI/публикация и installed/authenticated acceptance не подменяются.
Доказательства: private evidence/terminal-states-20261025/.

## Предыдущий кандидат: фокус каталога и прямой LoadingState — 25 октября 2026

UI pin `5f1271e387a1ae2dc933d7e14d5397df446092b4`, reference
`c33652802684ecea5e4dfd1fef4e14cf59daa8aa43710ebae4f02297469ac9f4`, версия0.3.2.

Пустой PageLoader удалён: Suspense напрямую использует LoadingState. Routes/admission сохранены; repository-configurations picker проверен с общим UI.

SearchableSelect Base сохраняет точку взаимодействия при чтении страницы и retry.
Pending скрывает старые данные и удерживает focus в Dialog; после ответа выбирается
доступное направление или recovery. Поиск пользователя и закрытие имеют приоритет.
Выбор значения и callback чтения не запускаются самим восстановлением focus.
PageNavigation сохраняет native disabled; локальные focus-копии не добавляются.
AST трёх роутеров подтверждает удаление только пустых обёрток с сохранением routes;
негативный контроль проверяет полный обход AST. Семь UI pins совпадают; шесть frozen
offline refresh сохранили locks/React singleton, SDK/skills pins не менялись.

Scoped gate25 stages прошёл:78 product cases (Tracker10, Wiki27, Forge29, Admin11,
Fleet1), Base28, guard32, reference83 и Workflow41 —262 unique cases без повторов.
Шесть TypeScript/source guards, examples, coverage, архив и SSR/Auth freshness PASS.
IAB24 адаптивных contexts и6 recovery contexts подтверждают три темы, standard/large,
320/375/1920/2560,40/44px и отсутствие overflow. Отдельно проверены приоритет поиска
и закрытие pending. Это source QA. Built Storybook, HTTP parity179, полный gate,
реальный zoom200, hosted CI и authenticated/installed приёмка остаются открытыми.
Публикация/runtime не выполнялись. Доказательства: private evidence/paging-focus-20261025/.

## Предыдущий кандидат: группы полей и подробности — 25 октября 2026

UI pin `c04047590f9931fcef39ad328512a22f07149934`, reference
`11c4fc27a5087eaecbcaaad822832729e6a5c434797e835cbe832acdf44df745`, версия0.3.2.

Две технические секции pipeline используют tone=muted; ошибка schedules — tone=danger size=sm. Записи и обработчики заданий не меняются.

Base владеет разделителем/плотностью Fieldset и вариантами appearance/tone/size
Disclosure. Внешняя компоновка, смысл, native children, open и действия остаются
у продукта. Удалены восемь presentation-копий; AST семи файлов подтверждает111
неизменных обработчиков и код вне перевода классов в props. Guard дополнительно
обнаружил compact-секцию Wiki; исправлена причина, правило не ослаблено.

Семь UI pins согласованы; шесть frozen offline refresh сохранили locks и React
singleton. SDK/skills pins не менялись этой волной. Scoped gate23 stages прошёл:
202 product cases (Tracker92, Wiki15, Fleet78, Forge17), шесть TypeScript/source
guards, reference82, настоящий архив и Workflow41. Отдельно Base8, SSR4 и guard32
прошли (всего369 unique cases этого среза, без сложения повторных прогонов).
IAB48 сочетаний React/SSR подтвердили три темы, плотность, standard/large и
320/375/1920/2560,40/44px, контраст и отсутствие overflow; отдельно проверены
FormData, disabled, сохранение ввода, Enter/Space и действие только по нажатию.
Это source QA. Built Storybook regression, HTTP parity177, полный gate, реальный
zoom200 и authenticated/installed приёмка остаются открытыми. Публикация и runtime
не выполнялись. Доказательства: private evidence/field-disclosure-20261025/.

## Предыдущий кандидат: разделы и выбранные строки — 25 октября 2026

UI pin `10896c8afcdd1227043f7290183f8044a0c21365`, reference
`7ba477baa2eda2f0e93bf58d14687717f64a9b72b7a955e4d9b1f390b9c9f14d`, версия0.3.2.

Обновлена только поставка UI; предметные компоненты Forge этой волной не меняются.

Общий Base оформляет разделители, технические блоки и выбранную строку.
Нативная семантика, выбранный ID, действия и внешняя компоновка остаются у продукта.
AST пяти изменённых файлов подтверждает63 неизменных обработчика. Расчёт брендинга
учитывает текст и статусы на поверхности выделения: исходный документ не меняется.
Семь UI pins согласованы; шесть frozen offline refresh сохранили locks и React singleton.
SDK/skills pins не менялись этой UI-волной; прежние upstream revisions сохраняются.

На итоговом кандидате прошли69 product cases (Admin8, Wiki15, Fleet46),22 branding
React,11 Node runtime/bootstrap и41 Workflow template cases; checked JS, examples,
reference/coverage, настоящий архив и SSR/Auth freshness. Шесть TypeScript и source
guards прошли на02ff0f78; публичные TS-контракты после него не менялись. IAB36
сочетаний трёх тем, трёх акцентов и320/375/1920/2560 подтвердили текстовый контраст
от4,51:1, marker, native row и отсутствие overflow; keyboard/pointer сохраняют focus.
Это source QA общей реализации. Built Storybook, HTTP parity173, полный gate,
реальный zoom200 и authenticated/installed приёмка остаются открытыми. Публикация
и runtime не выполнялись. Доказательства: private evidence/section-table-presentation-20261025/.

## Предыдущий кандидат: общий разделитель пагинации — 25 октября 2026

UI pin `7cef16712c453885952d16319439f0190f145747`, reference
`c6b48bda376ae0cc42303cf8d804036b6fd506dbfcff6eec51ab3f36bbb18a47`.

Три панели refs/tree/tags используют PageNavigation separated. Компактный режим,
выравнивание и callbacks обновления URL сохранены.

Base владеет разделителем и отступом из density tokens:4/8/12px. Обычный режим
остаётся без разделителя. Сверка AST пяти product files сохраняет128 handlers
и весь код вне className/separated. Новый общий guard обнаруживает aliases и
динамические className; внешняя компоновка остаётся разрешённой. Семь UI pins
согласованы; шесть frozen offline refresh сохранили lockfiles/React singleton.
SDK/skills pins неизменны. Scoped21 stages:90 product cases, шесть TypeScript,
source guards, reference81, настоящий архив, SSR/Auth/freshness прошли; Base28
и guard30 проверены отдельно (всего229 unique cases, без повторного сложения).
IAB48 contexts подтверждают темы, плотность, размеры320/375/1920/2560, крупный
текст, обычный соседний режим, pending и callback один раз при явном переходе.
Это source QA общей реализации; authenticated страницы продукта, built assets,
HTTP parity169, полный интегрированный gate и реальный zoom200 пока открыты.
Доказательства: private evidence/pagination-separator-20261025/. Новый standalone
пример: pdlc-ui example PageNavigation --case separated. Runtime не менялся.

## Предыдущий кандидат: выбор ресурсов Base — 25 октября 2026

UI pin `c3a7c578a1c1b21dd5759c0bd5c0f917e9b584b9`, reference
`a5638c1ff93a91b89a1b3f9849c1edced20a54a7fa38dc5874794f9414da3772`.

Принят общий UI-кандидат: честная постраничная выдача SearchableSelect и исправление
отступа выбранного пункта. Продуктовые запросы, права и callbacks не переносились.

Семь consumers используют один UI SHA; шесть frozen offline refresh сохраняют
lockfiles и React singleton. SDK/skills pins не менялись. Scoped gate830e41ea
прошёл24 стадии:156 product и36 Base cases, шесть TypeScript/source guards.
Для итогового CSS inputs повторены reference/archive/freshness и SSR/Auth;
JS test inputs подтверждённо неизменны. Повторные прогоны не суммируются.
IAB source QA:24 сочетания light/dark/gray × standard/large ×320/375/1920/2560;
40/44px, зазор8px, Escape/focus, URL/Back/Forward/reload, ошибки и выбранный ресурс
вне страницы. Синтетические GET-only данные; runtime/credentials не используются.
Это не authenticated приёмка продукта. Built Storybook regression, полный gate
новой волны, HTTP parity167 и реальный zoom200 остаются открытыми.
Доказательства: private evidence/search-filters-20261025/. Общий остаток — в Base
`docs/plans/shared-ui-consolidation-20261021.md`; публикация и установка отдельно.

## Предыдущий кандидат: общие части меню — 25 октября 2026

UI pin `1500913e136476d84aee90fbe38017b518f39d95`, reference
`cac88571998322babd4e11e42e7ce0614c673c6925fb23421b92ec3322926544`.

Account heading заменён DropdownMenuLabel; logout и видимость/права остаются у Forge.

Base владеет подписями, разделителем, метриками пунктов и Button size=icon.
Product callbacks, URLs, ACL и запросы остаются у владельца. Шесть frozen offline
refresh сохранили React singleton/версии и locks; SDK/skills pins независимы.
Scoped qualification: 128 product + 15 Base + 29 guard + 81 reference cases,
шесть TypeScript, examples/types, настоящий архив и SSR/Auth freshness прошли.
IAB общего меню: 24 сочетания темы/плотности/ширины; callback один, focus/40–44 px
и границы проверены. Это source QA, не authenticated или установленная приёмка.
Новый built Storybook, production assets, HTTP parity164, hosted CI и публикация
остаются открытыми. Доказательства: private evidence/menu-parts-20261025/;
общий оставшийся scope — в плане Base.

Предыдущая Button/Autocomplete волна отдельно завершила полный gate31/4290 и
HTTP parity160 на прежних exact heads; она не является новым полным прогоном
текущих menu inputs. Runtime/данные/credentials этим source-этапом не менялись.

## Предыдущий кандидат: компоновка Button — 25 октября 2026

UI pin `5338d65de4fb300df4b759f5865b2ee2fc92d1ce`, reference
`d7158acef7806c90d9d2a2967d22f795dde8b01826874b8073b0ca96df9967f6`.

Действие дерева файлов использует padding Base; ширина строки, отступ иконки и навигация остаются у Forge.

Base устранил перекрытие responsive hidden/display и выравнивания общим CSS.
Цвета, padding, радиусы и минимум 40/44 px принадлежат Base. Шесть изменённых
product AST сохраняют все данные/handlers вне className. Guard: 39 cases и
шесть source contracts; IAB: 168 пар общего Button. Это не приёмка backend.
Итоговый frontend/reference gate и built browser test пока открыты. SDK/skills,
runtime, данные и credentials не менялись. Прежняя квалификация имеет свой scope.

## Предыдущая волна: фирменные подсказки — 25 октября 2026

UI pin `56cf8a2f63b2b853103c164bbf1f7758a90e8db0`, версия 0.3.2; reference
`3d88aadb70081f614a1ef51481c32724c53bcd7c6135045d96bc7be2a1dfd1d4`.

Шесть полей refs/tag используют AutocompleteInput вместо datalist. Фильтры веток, payload и
trimming остаются у Forge. Прошли 39 scoped cases. Preview banner использует Notice Base; его
отдельный test gate учитывается отдельно.

Новый контрол и его popup принадлежат Base. IAB проверяет общую реализацию,
а не authenticated страницы продукта; прежние результаты сохраняют свой scope.
Scoped source/static прошёл: шесть strict TypeScript/CLI/choice consumers,
Base types, examples, reference/coverage, настоящий offline UI-архив, SSR/Auth.
Данные Radix подтверждают один layer stack у шести потребителей. Отдельно
прошли 26 Base, 104 product cases и 8 Forge preview cases. Полный frontend
gate и hosted CI этой волны ещё открыты; прежний полный PASS не подменяет их.
SDK/skills pins, установленный runtime и пользовательские данные не менялись.
Девять owner AGENTS уточняют выбор AutocompleteInput/Select/SearchableSelect.
Общий остаток ведётся в Base `docs/plans/shared-ui-consolidation-20261021.md`.

## Изменение

Браузер репозитория используют публичный Label вместо локального
оформления поля Git ref. Всего перенесено 1 подписей. Текст, htmlFor/id,
required, значение, обработчики, запросы и права сохраняет продукт.
Внешняя компоновка не переносится в библиотеку.

Первый срез использовал UI producer `8efc6c8d62b7d2c152b007215d6ce91f4c8f8d17` (0.3.1).
Label существовал ранее; нового API или зависимости для этих подписей нет.
SDK/skills pins сохранены; cached offline frozen install уже подготовленного
UI-пакета прошёл без обновления внешних версий.

## Проверка

Прошли 20 scoped test cases затронутых форм/экранов, scoped ESLint и
существующий primitive/choice contract. AST-сопоставление до/после не обнаружило
изменений вне импорта, тега подписи и её оформления.

Общий маршрут проверен в IAB на настоящей форме спринта Tracker: три темы,
standard/large и 320/375/1920/2560. Подписи имеют 14/16 px из BrandingScope
внутри Dialog Portal; htmlFor сохраняет focus и исходный callback получает
введённые данные. Это 24 source-composition проверки с синтетическим onSubmit,
не приёмка backend этого продукта. Полная новая сборка и hosted CI ещё открыты.

## Составные поля

Следующий срез заменяет обёртки: два FormField с публичным id SearchableSelect; настройки автозапуска и подключения сохраняют свои callbacks.
Текст, прежние handlers/value/required/disabled и продуктовые условия сохранены;
семантические JSX props сопоставлены по AST. Native label больше не содержит
одновременно поле поиска и потенциальную кнопку очистки. Вместо локальной
типографики используется готовый контракт Base, без нового публичного API.

Текущий UI pin — `948b492b36ff2c95215d43cf945f15a849d593f3`. Его 772 поставляемых файла совпадают с ранее
проверенным архивом; двоичные assets сравниваются без нормализации. Новый producer
добавляет только development guard. Поэтому пять неизменных установок сохранены,
а AI Hub обновляет только dependency URI: frozen offline install, один reused
package, без downloads и обновления версий. У шести потребителей подтверждены
CLI check и единый React peer graph. SDK/skills не менялись.

Прошли 5 scoped cases текущих затронутых экранов. Все пять React-владельцев
вместе с Fleet дали 210 уникальных product cases, отдельно Base — 44 contract
cases. IAB Admin подтвердил 24 сочетания темы/масштаба/ширины, native required,
min/max, focus и ввод; токен не выпускался. IAB AI Hub подтвердил 12 сочетаний,
открытие Select через подпись, сохранение выбора и обновление трёх примеров;
код не исполнялся, inference не запускался. Это проверки source-compositions
с синтетическим read-only транспортом, не authenticated runtime acceptance.

Новые frontend-сборки, полный интеграционный gate и hosted CI остаются открытыми.
Наличие development guard не объявляется доказательством визуальной полноты.

## Совместимое обновление локальных состояний Base

UI pin — 2e29b403b3987905494ed40fe812608cf998cc3a; reference
`e5cad6ebab4369dbd56831f566ab94141ca4de050b253f9b93191ddbcb75c23a`.
LoadingState/ErrorState и их общие композиции сохраняют page-scope по умолчанию;
local-scope доступен явно для отдельного блока. В этом владельце вызовы не менялись.
Cached frozen offline refresh обновил UI без downloads; остальные версии графа,
React singleton и SDK/skills pins сохранены. Installed CLI check прошёл.
AI Hub обновляет только URI immutable snapshot в package/lockfile; остальные
lockfiles не меняются. Прежние product tests не обозначаются новым прогоном.
Новый общий архив и standalone CLI проверены отдельно; полный frontend gate открыт.

## Текстовые ссылки и возврат

38 текстовых композиций в 18 файлах используют TextLink Base вместо повторов
цвета, hover и focus на NamespaceLink/anchor. Самостоятельный возврат в каталог
синхронизаций использует Button asChild variant=ghost size=icon с общей областью
нажатия. Namespace/query, href, target/rel, названия и обработчики сохраняются.
Ключи mapped-ссылок принадлежат внешнему TextLink; дополнительный anchor не создаётся.

123 уникальных scoped cases 11 файлов, TypeScript, ESLint и primitive/choice
guard прошли. AST всех 25 изменённых файлов Wiki/Forge не обнаружил изменений
непредставительных контрактов. Layout/содержимое/права и domain queries не
переносятся в Base. Прямой paint транспортных ссылок теперь обнаруживает
существующий source checker Base; его новый regression case сначала падал.

IAB настоящего WorkspaceLinks и ревизии Wiki с read-only адаптерами проверил
24 сочетания тем/палитр/ширин: общие цвета, focus, отсутствие вложенных anchors
и overflow, сохранение context при переходе. Это proof двух исходных композиций,
не полный route gate и не приёмка runtime. Собственные сервер и вкладка закрыты.
Непоставляемый source guard не объявляется принятым старым pin. UI producer,
reference, зависимости и SDK/skills не меняются; полный frontend gate открыт.

## Общая композиция простых полей

В этом срезе 19 повторных композиций подписи и одного контрола заменены
существующим FormField Base. Внешняя предметная компоновка и сложные группы
с несколькими контролами сохранены. Тексты и обязательность не меняются;
id/label association, все props данных и callbacks проверены AST для 25 файлов
пяти владельцев: 131 control и 111 подписей, включая нетронутых соседей.
Новые запросы, UI API и зависимости не добавлены. Проверенный UI producer —
2e29b403b3987905494ed40fe812608cf998cc3a; независимые SDK/skills pins сохранены.

Прошли 92 scoped cases этого владельца; вместе пять продуктов дали
320 уникальных cases. TypeScript и scoped ESLint пяти владельцев прошли.
IAB пяти реальных композиций с синтетическими read-only адаптерами подтвердил
120 сочетаний трёх тем, standard/large и 320/375/1920/2560. Подписи связаны
с контролами, высоты 40/44 px и отсутствие overflow сохранены. Дополнительно
проверены label click, выбор/focus и native required Tracker без записи.
Это source proof пяти композиций, не приёмка каждого маршрута и не zoom 200%.

Первая private-фикстура Wiki пропустила data.space.status: исправлен только
адаптер и повторены её сценарии. Этот fixture FAIL не объявляется продуктовым
дефектом. Собственные сервер и вкладка закрыты, неизменность исходников
браузерного среза подтверждена hash. Общий build/route gate, публикация source
и установленная платформа остаются открытыми. Независимое исправление SSR
брендинга Base квалифицируется отдельно от этого среза React-форм.

## Сведённый producer и локальные состояния

В этом срезе меняется только UI pin; прежние проверки owner source сохраняют
свой scope. Повторная backend/frontend-сборка без изменения их implementation
inputs не выполнялась. Полный согласованный gate остаётся открыт.

UI producer — d6c0914ae7bab37fc91b5f10602ad05543551ec0, reference
`5f4ff5f4ada180236c2d6e014b52e78ad60586824980c7b45316eb20b44a69a8`.
Frozen offline refresh прошёл без downloads и изменения lockfile; React singleton
и независимые SDK/skills pins сохранены. Настоящий архив Base проверен отдельно.
IAB использовал собственные исходники и read-only adapters; новый полный build,
hosted CI, zoom 200%, публикация и installed acceptance этим срезом не заявляются.

## Компактные сообщения и актуальная UI-поставка

ProtectedRoute показывает недоступное восстановление сессии через Notice
с прежним retry callback. AuthProvider продолжает владеть status/error/readiness;
анонимный fallback и новые сетевые действия не добавлены. 13 auth/route cases,
TypeScript и scoped ESLint прошли. IAB guard с синтетическим unavailable auth
проверил три темы и четыре ширины; error и доступный retry сохранены.

UI pin — eb3ceceeb1b1c5029ccaaa9b863d6646b35b1a46; reference
`5a702cb74a6eff5ec0e298ebdee609e70703e4fb01eda9e101cf7be84f019d36`.
Cached frozen refresh обновил UI без downloads и сторонних dependency upgrades;
React singleton и независимые SDK/skills pins сохранены. Настоящий UI-архив
проверен отдельно. Общий integrated gate, source publication и hosted evidence
остаются открытыми; установленный runtime этим изменением не обновляется.


## Списки и пагинация — 24 октября 2026

Шестнадцать ручных пагинаторов используют PageNavigation. Сохранены неизвестный итог репозиториев, cursor/offset, фильтры и владельческие счётчики. Дерево репозитория использует compact; общий fix Base обеспечивает 44×44 px mobile. Проверены 83 scoped cases, TypeScript и lint; IAB текущих страниц Forge этим этапом не объявляется.

UI producer `ff303ac5a273dc6b265292465b8b9db9a9476f9f`, fingerprint `206bf4889e50889e8c7bfab9aec36ef52df2e1e76dfcaa94a3ac756a7b80e2bf`.
Проверки относятся к исходникам собственных checkout; установка, hosted CI и общий
интеграционный gate сохраняют отдельный статус. SDK/skills pins не изменены.


## Сообщения, статусы и семантическая пустота — 24 октября 2026

Статус schedule использует Badge; ошибка попытки webhook и пояснение readback — Notice. DeliveryOpenButton сохраняет Base ghost и владельческую компоновку, без локального focus/hover. Проверены 19 scoped cases, TypeScript и scoped ESLint. Числа diff и краткая ошибка внутри summary-кнопки остаются владельческим содержимым, без вложенного div.

UI producer `56f27ee55bb0481ecc4733a0bb58144ae3cfed60`, reference `ba456575dd41f6237911ee97d0cec9de7c5f39941965714d3e4d86d09b0d8a2d`. Семь UI pins согласованы; шесть frozen offline refresh без downloads сохранили React singleton. SDK/skills pins не менялись.

Доказательства сохранены в private evidence/shared-message-audit-20261024/. Общий интеграционный gate, hosted CI, source publication и установленная приёмка остаются отдельными открытыми этапами.


## Общие заголовки и видимый фокус — 25 октября 2026

Production React source этой волной не менялся. Обновлён точный UI pin общего оформления focus-visible PageHeader; AI Hub дополнительно обновляет только URI immutable UI snapshot в package/lockfile. Прикладные unit/type результаты неизменных inputs переиспользуются, новый продуктовый browser PASS не заявляется.

UI producer `88e4ed54546181c6ffa46a38651b6553cd4e3aff`, reference `9d8689e5c5e52fd6bc3dd3d2b4b030bc992af26cfa22d626c15268765bb06316`. Семь UI pins согласованы, шесть frozen offline refresh без downloads и сторонних upgrades сохранили React singleton. SDK/skills pins неизменны. Настоящий UI-архив и offline CLI проверены вне workspace. Общий интеграционный gate, hosted CI, публикация веток и установленная приёмка остаются открытыми.


## Общие пары описания и показатели — 25 октября 2026

Свойства мобильного аудита, типовые порты/БД и описание переменных окружения используют DescriptionItem. Счётчики отчёта используют Metric/Badge; status distribution и исходные расчёты сохранены. Проверка «прочие = 2» обращается к именованной группе и definition вместо nextElementSibling. Переменные показаны как справочник, фактические env/secrets не читаются.

Проверены 11 scoped cases, TypeScript и scoped ESLint; общий UI guard сохранён. IAB использовал настоящие owner-компоненты с синтетическими read-only данными: общий срез четырёх владельцев — 240 сочетаний десяти композиций, трёх тем, standard/large и 320/375/1920/2560. Это не приёмка всех продуктовых маршрутов или backend.

UI остаётся 88e4ed54546181c6ffa46a38651b6553cd4e3aff / reference 9d8689e5c5e52fd6bc3dd3d2b4b030bc992af26cfa22d626c15268765bb06316. API Base, семь UI pins, SDK/skills pins и зависимости не менялись; install/build/export не повторялись. Собственный QA завершён без обращений к продуктовым API. Доказательства: private evidence/definition-pairs-20261025/. Общий integrated gate, публикация и installed acceptance остаются открытыми.

После исправления лишней вложенности трёх групп Forge их 24 UI-сценария квалифицированы отдельно; 216 сценариев остальных композиций переиспользованы по неизменным соответствующим inputs. Дополнительно проверена валидная структура dt/dd во всех десяти композициях. Новый семантический RED-тест прошёл после исправления; он не заменяет проверку значений. Финальный QA 55828 завершён, inputs неизменны и API requests отсутствуют.


## Общий возврат фокуса и presentation — 25 октября 2026

Branch/tag и breadcrumb actions используют готовые варианты Button без локального hover/font/size paint. Корень дерева использует size=icon. Focusable refs Table получает ring из Base; tabIndex, доступное имя и предметная ширина сохранены. В IAB выбор ветки и переход src → корень сохранили ref в URL. Scoped TypeScript, ESLint и 76 product cases прошли.

UI producer `ed8edd288f3980b521258bcac9082bfb88856b69`, reference `1424252c8cf384eb42385d5692daa148d0df30796f2a79cf100dd87e56e54743`. Семь UI pins согласованы; шесть frozen
offline refresh без downloads и сторонних upgrades сохранили React singleton.
SDK/skills pins неизменны. Настоящий архив UI содержит 780 файлов; offline CLI,
новые карточки и самостоятельные примеры проверены вне workspace без Storybook.

Общий IAB срез: 144 уникальных сочетания шести source-композиций, трёх тем,
двух type_scale и ширин 320/375/1920/2560. Из-за пропущенной утилиты временной
QA-страницы Table 24 сочетания перепроверены; 120 остальных сохранены после
доказанного сравнения CSS. Дополнительно 18 уникальных открытых Portal-сцен,
native Tab/ArrowRight таблицы и URL-навигация Forge. Продуктовые записи отключены
синтетическими adapters; это не backend или установленная приёмка. Снимок
ревизии Wiki и context drawer Fleet имеют unit, но не отдельный IAB proof этой волны.

Доказательства: private evidence/dialog-compositions-20261025/. Временные
процессы и IAB-вкладка закрыты, source inputs сохранены, API-запросов нет.
Общий интеграционный gate, HTTP parity, настоящий zoom 200%, публикация,
hosted CI и установленная приёмка остаются открытыми. Runtime не изменён.

## Общая типографика строк ресурсов — 25 октября 2026

Названия/подписи ресурсов используют Base; статус пайплайна использует Badge с явным адаптером queued/running/success/failed/canceled и нейтральным неизвестным состоянием. Labels, IDs, routing и trigger сохранены. Scoped cases: 34.

UI producer `1848c08d68f8a60f7dc179751984e8c821647373`, reference `250d3c802e3a22b4095efb70defe6b0baff379fd6b1c1f7bbcf9c10d2e30b983`. SDK/skills pins этой UI-волной не менялись; принятые upstream SDK-переходы сохранены. Семь UI pins согласованы. Frozen offline refresh шести React-потребителей сохранил locks и React singleton. Workflow получил штатный SSR export, Auth export не изменился.

IAB: 168 уникальных сочетаний Base и шести реальных source-композиций, трёх тем, двух type_scale и ширин 320/375/1920/2560 без overflow; семь снимков просмотрены. Изменённый Fleet проверен повторно вместо прежних 24 результатов. Данные синтетические, записи отключены; backend/установленная приёмка не заявляется. Собственные временные процессы и вкладка закрыты.

Доказательства: private evidence/remaining-ui-20261025/. Общий integrated gate, HTTP parity, zoom 200%, source publication, hosted CI и установленная приёмка остаются открытыми. Продолжается классификация других локальных компонентов и SSR/dev-preview.

## Общая offset-навигация — 25 октября 2026

В этой волне изменён только UI pin; runtime sources и backend не менялись. UI producer `97399e00e275c953c28a77d1b178a22a79ca2a93`, reference `48721f30d2841d0c76e086876340f5b5367953df4519118c9d92956b08c9f4cb`. Семь pins согласованы; frozen offline refresh сохранил locks и React singleton. SDK/skills pins неизменны. SSR/Auth assets проверены без byte-diff и повторного экспорта.

Base: 17 scoped cases; IAB Base/Admin — 120 уникальных сочетаний, пять просмотренных снимков, keyboard/URL transitions без записи. Архив и offline CLI проверены вне workspace. Это source proof с синтетическими GET-only данными; интеграционный gate, публикация, hosted CI и установленная приёмка остаются открытыми. Доказательства: private evidence/offset-navigation-20261025/.

## Интегрированная frontend-проверка — 25 октября 2026

Квалифицирован source head `8ff3718fc576ba75e04fc297ccf9e79f4a0e12c7`:
324 frontend cases, owner static/lint и production frontend build прошли.
UI producer `97399e00e275c953c28a77d1b178a22a79ca2a93`; dependency inputs,
SDK/skills pins и SSR/Auth exports этой проверкой не менялись.

ESLint исключает только приватный generated cache `.local/**`. Исходный FAIL
сохранён; production rules и проверка AppShell не ослаблены. Реальный ESLint
API подтвердил неизменный набор правил production-файлов.

Общий gate Base/справочника/шести frontend завершён: 29 стадий, 4270 test cases.
Это единый прогон; прежние scoped cases в него повторно не суммируются.
153 поисковых vectors совпали в JS, Rust HTTP API, offline и HTTP CLI.
Свежие built assets справочника проверены по bytes и через IAB.

Доказательства: private `evidence/ui-integrated-20261025/`; общий статус и
оставшиеся переносы — [план Base](https://github.com/FerrPOINT/services-base/blob/refactor/shared-ui-residuals-20261024/docs/plans/shared-ui-consolidation-20261021.md).
Docs-only фиксация результата не пересобирает прежние inputs. Hosted CI,
source publication и установленная приёмка здесь не заявляются. Реальный zoom
200% не применился доступной IAB-командой; CSS/DPR-подмена не использована.
