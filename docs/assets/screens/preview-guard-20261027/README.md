# Общая защита ухода

Codex IAB, 2026-10-27. Настоящий RepositorySyncPreview и извлечённый из main.tsx
PreviewEntrypoint, Base UnsavedChangesGuard и owner i18n. Окружение, данные,
переход за пределы source-сцены и transport синтетические. API/Auth и установленные
стенды не используются; fixture не входит в production-поставку.

- [375 px](forge-dark-375.png).
- [1920 px](forge-dark-1920.png).
- [2560 px](forge-dark-2560.png).

Full-page снимки сохраняют заголовок source-проверки. Указана фактическая ширина
iframe; внешний canvas может быть шире. Все три снимка просмотрены, диалог
проверен на устойчивой геометрии. Матрица light/dark/gray × 320/375/1920/2560
проверяет Enter/Escape, возврат focus и отсутствие overflow. Отдельно проверены
внутренняя навигация, скрытый черновик другого репозитория, сохранение/сброс и
подтверждённый уход из списка. Это не квалификация production synchronization.
