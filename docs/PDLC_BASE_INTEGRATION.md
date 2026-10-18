# PDLC и интеграция с Base

Канонические определения находятся у Base: [TERMS](https://github.com/FerrPOINT/services-base/blob/02e1ebbd3d2fcbf1c5e0f55a39d3cf554b085217/docs/platform/TERMS.md),
[ADR 0005](https://github.com/FerrPOINT/services-base/blob/02e1ebbd3d2fcbf1c5e0f55a39d3cf554b085217/docs/adr/0005-pdlc-base-sdlc-terminology.md),
[BRANDING](https://github.com/FerrPOINT/services-base/blob/02e1ebbd3d2fcbf1c5e0f55a39d3cf554b085217/docs/BRANDING.md). Документы продукта ссылаются на эти определения.

## Ответственность продукта

Продукт владеет: Git, workspaces, проверка и поставка. Запросы, DTO, маршруты, права и предметные правила
остаются здесь. Общие компоненты и механизмы получают данные и callbacks.
SDLC API/scopes/roles/workflow IDs сохраняются как предметные контракты.

## Воспроизводимые зависимости

Rust crates `pdlc-*` подключаются из соседнего Base по `.namespace-base-revision`; `.base-revision` сохраняет совместимый fallback. Оба SDK pin в данном комплекте совпадают, но не зависят от skills/catalog pin.

React импортирует публичный API `@pdlc/ui`; library source расположен в `base-modules/ui`. Из frontend используйте `pnpm exec pdlc-ui search "выбор ресурса"`, затем `show SearchableSelect` и `example SearchableSelect`. CLI не требует Storybook или сети.

UI SHA хранится в `.base-ui-revision`; пути package/lockfiles, Docker и CI
используют `services-base/base-modules/ui`. Приложения `components-dictionary`
и `base-platform` расположены отдельно и не входят в UI-пакет продукта.
Визуальный каталог запускается только при необходимости проверки примера.

## Переходные контракты

Общий SDK распознаёт оба PAT-префикса и явно доверенную пару JWT audiences.
Проверки issuer, подписи, срока действия и предметных прав сохраняются.
Канонические общеплатформенные env-переменные используют `PDLC_*`; временные
`SDLC_*` aliases не должны конфликтовать. Собственные env-префиксы продукта
сохраняются. Значения credentials не выводятся при диагностике конфликта.

SSO readers поддерживают прежние транзакции. Новые writers включаются
отдельной установкой после принимающих сторон. Опубликованный пользовательский
branding сохраняет приоритет над default PDLC. Исторические ADR, evidence,
миграции и идентификаторы данных не переписываются ради имени платформы.

Локальные source-тесты, hosted CI, установка и authenticated browser-приёмка
фиксируются отдельно. Этот source-комплект не меняет установленный runtime,
credentials или группы `pdlc1`, `pdlc2`, `pdlc3`, `pdlc-common`.

## Справочник и общий каркас: 2026-10-18

UI pin `198169e1c01bd7d4554aa2b7cfbc272c3014e0b0` обновляет подготовленный справочник, проверяемые API-примеры и инструменты проверки Base. Общий CSS ограничивает ширину ProjectPicker и переносит длинные идентификаторы ResourceList; логика React, HTTP API, SDK и skills не меняются. SSR CSS и fingerprint получены штатным exporter. CLI работает offline: `pdlc-ui check` в начале задачи или после смены pin → `search "задача"` → `show ExportName` → один `example`. Полный fingerprint не вычисляется на каждом поиске.

Карточки и демонстрации поддерживаются [приложением Base](https://github.com/FerrPOINT/services-base/tree/main/components-dictionary). Общие каркас, навигация, темы и мобильное меню принадлежат Base; запросы, DTO, права и предметные состояния остаются у продукта. Приёмка source-кандидата не означает установку или production-приёмку.

`pnpm --dir frontend shell:check` входит в `lint` и проверяет использование AppShell и отсутствие локальной копии его геометрии. Предметные панели внутри содержимого и их стили допустимы. Проверка использует закреплённый TypeScript продукта; отдельный install инструментария Base не нужен.
