# Platform shell evidence — 2026-09-21

Проверен настоящий `AppShell` с экраном настроек и in-memory QA session. Временный preview-entry после проверки удалён; production auth и API не изменялись.

## Матрица

| Viewport | Темы | Ожидаемая навигация |
|---|---|---|
| 375×812 | light, dark | mobile header + drawer 320 px |
| 768×900 | light, dark | rail 72 px |
| 1440×900 | light, dark | sidebar 264 px |
| 2560×1200 | light, dark | sidebar 264 px |

Для каждой комбинации получено: header 60 px, horizontal overflow 0 px, `aria-current="page"` у настроек, 0 видимых controls меньше 40×40 px, 0 кнопок без доступного имени, 0 console/page errors.

Drawer дополнительно проверен на focus trap через `Shift+Tab`, закрытие по Escape и возврат фокуса trigger. Машиночитаемые результаты находятся в `qa-results.json`.
