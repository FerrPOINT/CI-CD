# Forge PDLC2: выбор проверок и поставляемых targets

Configuration only: baseline e1ce594d. Общие правила принадлежат Base
BUILD_PERFORMANCE/TESTING, SDK/UI/skills/operator/quality pins независимы.

1. Подключить exact Base selector и product descriptor. Runtime targets —
   cicd-server/forge-runner; CLI, delivery, migration tools остаются явными целями.
   Их source inputs и фактические local package edges определяют consumers.
2. Automatic CI/Root/raw helper используют Base plan. Docs/test-only не создают
   application artifact. Recipe-only выбирает упаковку соответствующего сервиса.
3. Старый full CI и git_receive_only сохранить в reusable manual qualification,
   full release вызывает его один раз. PostgreSQL/Git/security/native QA gates
   не исключать; actual manual execution отделить от source proof.
4. Разделить Vite и Vitest configuration, сохранив test config typing/workers.
   Уточнить Tailwind roots до test-only exclusions; unit/types/compile по одному разу.
5. Сохранить dependency layers и timestamp reset. Runtime build не компилирует
   tools вне runtime image. Contexts включают все embedded product/Base helpers.
6. Scoped source checks/Root no-change/PlanOnly; native artifacts, old timestamps,
   cache/handoff/recovery, CPU/RAM/I/O и live qualification — отдельная веха.

Вариант второго локального path classifier отклонён: selector принадлежит Base.
Не менять Cargo/Node dependencies, source Rust/TSX, runtime или данные.

## Уточнение 2026-11-01: workers frontend и входы упаковки

В обоих frontend Dockerfiles включить существующий официальный Nginx entrypoint
autotune по CPU quota/cpuset. Не менять версию image, builder и штатный entrypoint.
Флаг требует writable /etc/nginx/nginx.conf; read-only/non-root deployment
проверяется отдельно. Source-изменение не означает обновление runtime.

Проверка этой правки — scoped selection/static, без build/deploy. При отдельной
runtime-вехе проверить exact image, effective worker_processes, отдачу ресурсов
и latency/error/restart counters в Docker/Compose и Kubernetes; startup feature
не считается доказательством уменьшения RAM или устранения зависаний.
Канонические измерения и общие правила принадлежат Base, PERFORMANCE_SPEC,
PERF-019. Существующие runtime и независимые pins не переключать.
