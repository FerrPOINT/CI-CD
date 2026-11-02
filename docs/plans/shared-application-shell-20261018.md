# Подключение общего каркаса Base

Статус: реализовано и проверено в source; установка runtime отдельна.

Канонический контракт — [план Base](https://github.com/FerrPOINT/services-base/blob/main/docs/plans/shared-application-shell-20261018.md).

Проектная навигация, scope query, permissions и delivery sections остаются в Forge. Ключ forge:sidebar-compact сохраняется; локальные drawer и effect хранения удаляются.

Проверки: frontend unit/type/build, настоящий packed UI, права/прямые ссылки,
compact и mobile navigation. Точный UI pin находится в .base-ui-revision.
Установка runtime, backend и SDK/skills pins не входят в этот source-этап.

Локальные переопределения общих accent-токенов удаляются. Дополнительные
предметные semantic aliases ссылаются на канонические цвета Base.

Точный UI-кандидат: `4b80fbfbfae48d95219011a4612dd9427a1a4b92`. Связанные проверки и ограничения
фиксируются в [отчёте Base](https://github.com/FerrPOINT/services-base/blob/main/docs/reports/shared-application-shell-20261018.md).

## Коллекции навигации Base — source qualification, 26 октября 2026

2 native контейнера используют SidebarList Base вместо внутреннего
CSS-класса и повторённого separator paint. Исходные nav/list/list-item,
keys, hidden, aria, маршруты и callbacks сохраняются; полный скомпилированный
модуль сравнен с baseline после нормализации только объявленных wrappers.
Для Wiki это включает существующие коллекции дерева документов; группировку,
каталог, права, URL и раскрытие Base не забирает. Внешние margin сохранены.

Проверены22 widget/shell cases без изменения test assertions, TypeScript,
lint и UI guard. Общие20 Base cases проверяют native semantics, hidden,
focus, группировку и AppShell;49 guards запрещают копии layout и обращение
к внутреннему base-sidebar-list, включая alias класса. Source IAB:240
сочетаний реального Base AppShell, групп и самостоятельного примера,
22 клавиатурных действия, три темы, три плотности, два масштаба типографики
и320/375/1920/2560. Native ul/li и доступные имена сохранены; drawer наследует
плотность, Escape возвращает focus. Три screenshot просмотрены; console
чиста, source-сервер закрыт без API-запросов. Это общий UI с синтетической
навигацией; реальные product widgets проверены unit и source equivalence,
не полной authenticated browser-приёмкой. Реальный zoom200% не заявляется.

Точный UI producer и installed package qualification фиксируются после
Base commit. SDK/skills не меняются; source adoption не означает установку.
Evidence:private sidebar-list-consolidation-20261026.

Для этого переноса принят UI producer `00f1659c8f49c83cdf3b157125945ddaecf3beff`. Шесть offline frozen
installs подтвердили source/reference, CLI и единый React; frontend types,
shell/choice guards и штатные SSR exports прошли. UI pins семи потребителей
согласованы; SDK/skills сохранены. Архив UI проверен вне workspace без сети
и Storybook. Это source adoption; установленный runtime не обновлялся.
