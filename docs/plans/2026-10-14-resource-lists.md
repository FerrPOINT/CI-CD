# Общие списки ресурсов Base

Утверждённый объём: одинаковые отступы, разделители, hover/focus обычных списков; продукт сохраняет данные, раскладку, маршруты и действия. Backend/skills pins не меняются.

## Контракт

Боковой отступ — 16 px. Normal: вертикальный 8 px, минимум 48 px; comfortable: 16 px, минимум 56 px. Высота не фиксируется. Семантика нативная; контейнер с несколькими действиями не становится кнопкой. CSS Base используется React и штатным SSR-экспортом.

## Покрытие

| Участок | Результат | Основание |
|---|---|---|
| `frontend/src/pages/artifacts/index.tsx:107` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/audit-log/index.tsx:190` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/compare/index.tsx:102` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/delivery-configurations/index.tsx:30` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/environments/index.tsx:142` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/environments/index.tsx:251` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/pipeline-detail/index.tsx:854` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/pipelines/index.tsx:130` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/projects/index.tsx:46` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/pull-request-detail/index.tsx:209` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/pull-requests/index.tsx:429` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/repository-browser/index.tsx:306` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/repository-browser/index.tsx:519` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/repository-browser/index.tsx:892` | Исключено | Специализированный файловый браузер |
| `frontend/src/pages/repository-browser/index.tsx:1012` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/repository-browser/index.tsx:1142` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/runners/index.tsx:142` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/schedules/index.tsx:182` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/secrets/index.tsx:213` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/settings/index.tsx:93` | Исключено | Описание свойств, не коллекция ресурсов |
| `frontend/src/pages/webhooks/index.tsx:116` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/webhooks/index.tsx:294` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/webhooks/index.tsx:370` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/pages/webhooks/index.tsx:494` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/widgets/execution-list.tsx:287` | Перенесено | Обычный список ресурсов или активности |
| `frontend/src/widgets/repository-configurations.tsx:178` | Перенесено | Обычный список ресурсов или активности |

Сайдбары, document trees, маркированные пояснения/ошибки, формы, редакторы, таблицы и схемы исключены по назначению, а не по совпадению CSS-класса.

## Приёмка

Компонентные тесты и Chromium contract: семантика/действия, 16 px, grid, темы, длинный текст, 375/1281/1920 px, native disclosure. Затем продуктовые тесты, lint, typecheck/build, packed SDK и SSR drift. Установленная PDLC1 проверяется через IAB; происхождение images фиксируется отдельно от source/CI. Изменённые сервисы обновляются через private bundle с rollback images.
