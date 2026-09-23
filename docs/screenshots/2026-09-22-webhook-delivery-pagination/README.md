# Проверка истории доставок webhook

Production bundle проверен в Chromium через общий CI/CD shell и настоящий
Central Auth SSO. Ответы только для списка, detail и requeue доставки были
подменены детерминированными fixtures: 43 записи и три ожидаемых POST requeue.
Рабочие проекты, webhook-и и доставки не изменялись.

Проверены 39 состояний в темах `light`, `gray` и `dark` на ширинах 375, 768,
1280, 1920 и 2560 px. Матрица покрывает первую и вторую страницы, status/channel
filters, прямой URL страницы, отсутствие результатов, initial 503, локальный
retry, detail attempts и requeue success toast. Получено 14 контрольных
скриншотов.

Итог: 0 horizontal overflow, 0 неожиданных console/network/runtime errors и
0 serious/critical axe findings. После показа toast проверка выполнялась на
settled state через 1 секунду; вычисленные foreground/background во всех трёх
темах соответствовали теме, opacity равнялась 1.

Снимки:

- `deliveries-dark-375-page1.png`
- `deliveries-dark-1920-page1.png`
- `deliveries-gray-375-page1.png`
- `deliveries-gray-1920-page1.png`
- `deliveries-light-375-page1.png`
- `deliveries-light-768-page1.png`
- `deliveries-light-1280-page1.png`
- `deliveries-light-1920-page1.png`
- `deliveries-light-2560-page1.png`
- `deliveries-light-375-page2-filtered.png`
- `deliveries-light-1920-page2-filtered.png`
- `deliveries-light-1280-no-matches.png`
- `deliveries-light-1280-error.png`
- `deliveries-light-1280-requeued.png`

Это browser QA production frontend с настоящим SSO, но с изолированными
delivery API fixtures. Backend page contract отдельно проверен unit-тестами и
интеграционным тестом на PostgreSQL 17.
