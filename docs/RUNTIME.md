# Runtime Forge CI/CD

## Компоненты и порты

Runtime состоит из трёх Docker Compose сервисов: PostgreSQL, Rust API и React Dashboard, отдаваемого nginx в production-образе.

| Компонент | Порт хоста по умолчанию | Переменная |
|---|---:|---|
| API | `22801` | `CICD_API_PORT` |
| Dashboard | `22802` | `CICD_WEB_PORT` |
| PostgreSQL | `22543` | `CICD_DATABASE_PORT` |

Backend слушает адрес из `CICD_BIND`; по умолчанию это `0.0.0.0:22801`. В development Vite проксирует `/api` на `http://localhost:22801`, а в Docker Compose frontend обращается к API через опубликованный маршрут приложения.

## Конфигурация

Конфигурация загружается только из переменных окружения с префиксом `CICD_`. Значения, применяемые текущим compose/runtime:

| Переменная | Назначение |
|---|---|
| `CICD_DATABASE_URL` | URL подключения backend к PostgreSQL; обязателен для запуска API. |
| `CICD_BIND` | адрес и порт Rust API. |
| `CICD_DATABASE_USER` | пользователь PostgreSQL при запуске compose. |
| `CICD_DATABASE_PASSWORD` | пароль PostgreSQL при запуске compose; не коммитить production-значение. |
| `CICD_DATABASE_NAME` | имя базы данных. |
| `CICD_DATABASE_PORT` | внешний порт PostgreSQL. |
| `CICD_API_PORT` | внешний порт API. |
| `CICD_WEB_PORT` | внешний порт Dashboard. |

`RUST_LOG` используется для фильтрации `tracing`, но не является переменной доменной конфигурации `CICD_`. Локальное окружение создаётся из `.env.example`; файл `.env` не должен попадать в Git.

## Последовательность старта API

Текущий backend выполняет старт строго в следующем порядке:

1. Инициализирует `tracing_subscriber` с `RUST_LOG`.
2. Читает `CICD_DATABASE_URL` и `CICD_BIND`.
3. Создаёт `sqlx::PgPool` к PostgreSQL.
4. Вызывает `store::migrate()`; текущая схема создаётся идемпотентно через `CREATE TABLE IF NOT EXISTS`.
5. Привязывает `tokio::net::TcpListener` к `CICD_BIND`.
6. Создаёт Axum router с `AppState`, содержащим пул, и запускает `axum::serve`.

Если соединение с БД, миграция или bind listener завершаются ошибкой, процесс не начинает обслуживать HTTP. Это исключает состояние, в котором API принимает запросы без готового хранилища.

## Жизненный цикл процесса

После старта API обслуживает REST-маршруты `/api/v1/*`, включая health-check. Docker Compose ожидает health PostgreSQL перед запуском backend и health backend перед запуском frontend. Операционные команды:

```bash
docker compose up --build -d
docker compose ps
docker compose logs -f backend
curl -fsS http://127.0.0.1:22801/api/v1/health
docker compose down
```

Для пересоздания образа или контейнера использовать `docker compose up -d` или `docker compose up --build -d`, а не `docker compose restart`: restart не применит обновлённый образ и конфигурацию.

## Сигналы и graceful shutdown

Текущая точка входа завершает `axum::serve` при завершении процесса, но явный graceful shutdown ещё является целевой доработкой. Она должна обрабатывать `SIGTERM` (Docker/Kubernetes) и `SIGINT` (интерактивный запуск) и передавать единый сигнал остановки через `tokio::sync::oneshot`.

Целевая схема:

```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
tokio::spawn(async move {
    wait_for_sigterm_or_sigint().await;
    let _ = shutdown_tx.send(());
});

axum::serve(listener, app(Some(pool)))
    .with_graceful_shutdown(async { let _ = shutdown_rx.await; })
    .await?;
```

Обработчик сигнала должен остановить приём новых соединений, дать текущим HTTP-запросам завершиться в пределах deadline и затем освободить ресурсы. С добавлением runner-ов та же отмена должна прекращать диспетчеризацию новых job и завершать фоновые задачи без потери состояния из PostgreSQL. Детали таймаутов и восстановления описаны в `docs/RESILIENCE.md`.

## Проверка runtime

Перед передачей развёртывания проверить конфигурацию и жизненный цикл:

```bash
docker compose config
docker compose up --build -d
curl -fsS http://127.0.0.1:22801/api/v1/health
docker compose ps
docker compose down
```

## Связанные документы

- `README.md`
- `docs/RESILIENCE.md`
- `docs/STORAGE.md`
- `docs/ARCHITECTURE.md`