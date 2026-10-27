# Принятие общего оформления полей выбора

## Объём

Принять UI pin `03a35d80e7d10dce5ec4cf1df07860b7bca4fa72` из Base main ([#371](https://github.com/FerrPOINT/services-base/pull/371)).
Select, MultiSelect, ProjectPicker и серверный adapter используют общие menu styles;
поисковый выбор сохраняет Dialog, callbacks, paging и правильное выравнивание строк.
Продукт не добавляет копии CSS или собственные правила выбора.

Переход от прежнего pin включает уже квалифицированные DiceBear 10 и общий
application-access runtime. Публичные props и правила доступа сохраняются;
локальные проверки должны покрывать адаптеры/guards продукта и сборку.
Обновить frozen lockfile (React) или штатный SSR export (Workflow).
SDK, backend, схемы, данные, роли и права не меняются. Merge и установка — разные этапы.

## Приёмка

Проверить точный UI source, reference/package integrity, frozen install с прежними
строгими peers и engines, product typecheck, адресные UI/access tests и frontend
build; для Workflow — целостность SSR export и адресные template/choice checks.
Браузерная проверка общего Base покрывает три темы, single/multiple, поиск,
reset, scrolling и focus; продуктовая приёмка дополняет её, не заменяет.
Точные результаты и установленное состояние — в workspace delivery report.

Frontend baseline обновлён с Node 22.20.0 до 22.22.2: уже принятый
react-router 8.4 требует >=22.22.0. Strict engines/peers сохраняются; backend
toolchain и SDK pins не меняются. Node 26 у Admin остаётся прежним.
