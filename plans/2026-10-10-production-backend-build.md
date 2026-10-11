# Исправление production-сборки backend

**Статус 2026-10-10:** Current verified; обе реальные Docker-рецептуры
собраны на закреплённом Base. Итоговая поставка и условия merge проверяются отдельно.

В рамках утверждённого циклического ревью реальная umbrella-сборка выявила
ошибку: Cargo.toml объявляет `forge-delivery` и `forge-pg-migrate`, но слой
кеширования зависимостей создаёт только прежние binary sources. Cargo прекращает
сборку до копирования настоящих исходников. Native Rust checks этот слой не
выполняют. После исправления временных целей реальная компиляция также
обнаружила отсутствующие `include_bytes!` inputs: продуктовый postgres-delivery.py
и три проверяемых Base helper.

## Изменение

Согласовать временные binary sources в `backend/Dockerfile` и
`backend/Dockerfile.umbrella` с объявленными Cargo targets. Удалить неиспользуемый
dummy.rs: `autobins = false`, такого target в Cargo.toml нет. Сохранить locked
сборку, package cleanup и последующее копирование настоящих sources. Добавить
в build context только четыре необходимых compile-time script witness файла
из того же закреплённого набора исходников; проверки их bytes сохраняются.

Контракты API, данные, migrations, права и runtime configuration сохраняются.
Новый ADR не нужен: существующая схема кеширования и поставки сохраняется.

## Проверки

- Воспроизведённый отказ исходной production-сборки с отсутствующими файлами.
- Обе реальные Docker-рецептуры собираются из frozen candidate source и
  согласованного Base; build не меняет Cargo.lock.
- Production image содержит настоящие server/runner ELF после cleanup/rebuild.
- Rust regression evidence сохраняется только при точном совпадении исходников
  и manifests; документационные гейты выполняются заново.
- Финальные pins/images/live, обязательные CI и три чистых полных ревью остаются
  условиями merge. Установленный runtime не переключается.
