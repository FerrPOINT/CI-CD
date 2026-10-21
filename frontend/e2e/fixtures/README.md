# Изолированная регрессия истории синхронизации

`repository-sync-history.html` — отдельный source-only entrypoint Vite. Он использует
настоящий `RepositorySyncEditor`, BrowserRouter и закреплённый общий UI; основной
`index.html` и production build его не включают. Playwright подставляет только
явные synthetic GET-ответы пустой истории, запрещает любые API-записи и внешнюю
сеть. SSO, credentials, установленный backend и провайдеры не используются.

Из `frontend`, на Node 22.20.0 и pnpm 10.28.1 после frozen install:

```powershell
pnpm exec tsc --project tsconfig.sync-history.json
pnpm exec playwright test --config playwright.sync-history.config.ts
```

Отдельный config запускает собственный Vite на `127.0.0.1:18935`, запрещает reuse
чужого сервера и закрывает процесс после проверки или ошибки. Для другого свободного
loopback-порта можно задать `E2E_BASE_URL`; установленный runtime указывать нельзя.
Без установленного движка Playwright сообщает ошибку; config не скачивает браузеры.

Тест выполняется в Chromium, Firefox и WebKit при 375/1026/1440/1920/2560 px. Проверяются
настоящий pointer focus + mousedown, ровно один history PUSH, Back/Forward,
повторный выбор после Back, Enter без повторного перехода, автоматическая активация
стрелками, reload и сохранение фильтров. Screenshots и failure traces лежат в
`test-results/sync-history`.

Обычный production E2E config пропускает эту отдельную fixture-регрессию. Явный
`playwright.sync-history.config.ts` включает её и не допускает тихий all-skip.
Успешная source-проверка не заменяет приёмку установленного приложения с настоящим SSO.
