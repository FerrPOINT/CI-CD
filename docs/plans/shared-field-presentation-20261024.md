# Общие подписи полей Base

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
