# Runbook: Observability (K6.2)

Профиль `observability` в `docker-compose.local.yml` поднимает Prometheus (7790) и Grafana (7791) для Forge CI/CD.

```bash
docker compose -f docker-compose.local.yml --profile observability up -d
```

- Prometheus: http://127.0.0.1:7790 (scrape `forge-cicd` → `cicd-backend:22801/metrics`, rules из `deploy/observability/forge-alerts.yaml`)
- Grafana: http://127.0.0.1:7791 (anonymous, dashboard `Forge CI/CD — Base Platform` провижинится из `deploy/observability/forge-dashboard.json`, datasource → Prometheus)

## Alerts

### ForgeBackendDown
`up{job="forge-cicd"} == 0` 2m.
1. `docker ps | grep cicd-backend` — контейнер жив?
2. `docker logs sdlc-local-cicd-backend-1 --tail 50`
3. `curl -s http://127.0.0.1:7711/api/v1/health` — если 200, а scrape падает: проверь сеть `sdlc-local_cicd` (`docker network inspect`), /metrics не требует auth.

### ForgeHttp5xxSpiking
`increase(forge_http_5xx_total[15m]) > 10`.
1. `docker exec sdlc-local-cicd-postgres-1 psql -U cicd -d cicd -Atc "SELECT count(*) FROM execution_attempts WHERE error_tail LIKE 'internal%' AND created_at > now() - interval '15 minutes'"`
2. Лог backend с request_id из ошибок API; типовые причины — миграционный дрейф схемы (колонка добавлена, а SELECT старый) и sqlx-таймауты под нагрузкой.
3. Если 5xx идут от runner-маршрутов — см. `docs/runbooks/` CI_CD.

### ForgeJobsFailing
`forge_jobs_failed_24h > 20` 15m.
1. UI: Jobs → фильтр failed; или CLI `cicd-cli job attempts --id ...`.
2. Логи попыток: `GET /api/v1/attempts/{id}/logs/page`.
3. Частые причины: seccomp-профиль запретил syscall (EPERM/ENOSYS в error_tail — добавь в `deploy/forge-job-seccomp.json`, пересоздай cicd-backend), OOM по resource class (поднять `CICD_RUNNER_RESOURCE_CLASS`), инвалидный `.forge-ci.yml` (400 от parser).

### ForgeQueueStuck (critical)
queued > 0 и runners online = 0.
1. `docker logs sdlc-local-cicd-backend-1 | grep -i runner`
2. Embedded runner: env `CICD_EMBEDDED_RUNNER_ENABLED=true`? docker-сокет смонтирован?
3. `docker exec sdlc-local-cicd-postgres-1 psql -U cicd -d cicd -Atc "SELECT name,status,draining,disabled_at FROM runners"`
4. Если runner draining — снять флаг через UI/CLI (`cicd-cli runner update --drain false`).

### ForgeNoRunnersOnline
30 минут 0 онлайн. Проверить heartbeat-цикл runner'а (outbox/listener в логах), затем как ForgeQueueStuck.

## Изменение правил/дэшборда
Правила и дэшборд монтируются read-only; после правки — `docker compose restart prometheus grafana` (с профилем). Алерты без Alertmanager пока видны только в UI Prometheus → Alerts.
