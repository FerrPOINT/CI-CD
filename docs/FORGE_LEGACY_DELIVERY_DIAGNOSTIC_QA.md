# Targeted legacy delivery QA

Этот пакет проверяет только legacy local CLI recovery. Новый HTTP test не меняется.
Причина прежнего timeout не установлена: marker/wchan/stack нужны для следующего
наблюдения, а не для вывода о ресурсах. CLI deadline остаётся 25 секунд.

## Источники

- `9948d5e` main содержит diagnostics из worker `019c044` без смысловых отличий.
- Исходники Forge и Base экспортировать из immutable Git SHA, без dirty файлов,
  `.local`, `target` и credentials. Base SHA брать из `.namespace-base-revision`
  того же Forge export; ожидаемый сейчас `33ce7042a7617db59285390a67b191e3ae738dbf`.
- `FORGE_SOURCE_SHA` задаётся до Cargo: `option_env!` захватывает его при компиляции
  integration test. При запуске сохранить ту же переменную. Runtime-only env не
  привязывает уже собранный test binary к исходникам. Сценарий отвергает mismatch,
  malformed SHA и наличие переменной только на одной стороне.
- Без обеих переменных обычный developer run допускается, но печатает
  `DELIVERY_SOURCE_SHA=null` и не является source-bound QA.

## Команда

В existing временном Forge Compose QA helper, из immutable `CI-CD/backend`,
заменить broad stages только следующей командой. Не запускать прежний полный
`forge-qa/checks.sh` ради этого selector:

```bash
cargo test --locked --offline -p cicd-server --features integration --test integration_db \
  sdlc_workspace::task_delivery::sdlc_task_delivery_actual_versions_health_acceptance_rollback_and_crash_recovery \
  -- --exact --nocapture --test-threads=1
```

Для отдельной проверки трёх diagnostic unit tests без подключения PG:

```bash
cargo test --locked --offline -p cicd-server --features integration --test integration_db \
  sdlc_workspace::task_delivery::timeout_diagnostic:: -- --nocapture --test-threads=1
```

Не использовать `--ignored`: эти tests не ignored. Требовать ровно один passed
legacy test; ноль selected tests не является PASS. Diagnostic tests компилируются
в том же integration binary и проверяют numeric/secret-free projection,
kernel-symbol filtering и compile/run source-binding policy.

## Environment

Обязательны `FORGE_SOURCE_SHA` (canonical lowercase 40 hex, точный export SHA)
и `CICD_TEST_DATABASE_URL` (собственная disposable PG17, право создавать/удалять
test schemas). DB secret читать из private mounted file, не печатать connection
string. Для existing helper также сохранить `FORGE_SDK_SHA`, locked registry,
`CARGO_HOME`, disposable `CARGO_TARGET_DIR`, `TMPDIR`, `CARGO_NET_OFFLINE=true`,
`CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`. Toolchain: Rust 1.88.0; нужны `git`, `curl`, `sh`.

Для минимального selector **unset `CICD_TEST_DELIVERY_ORIGIN`**: fixture сама
собирает `tests/helpers/manifest_target.rs` pinned rustc из того же export и
запускает loopback target с собственным temporary root. Если main сохраняет
existing Compose target, допустимо только `http://delivery-target:8080` и общий
`/delivery-qa` mount; target helper должен быть собран из того же Forge SHA.
Оба пути уже поддержаны fixture; новый постоянный стенд не нужен.

`CARGO_BIN_EXE_forge-runner`/`CARGO_BIN_EXE_forge-delivery` задаёт Cargo, вручную
их не подменять. Остальные child CLI credentials/config задаёт existing fixture;
ambient production tokens не требуются.

## Evidence и очистка

Сохранить private log с `DELIVERY_SOURCE_SHA` и, при timeout,
`DELIVERY_TIMEOUT_STATE`. Последний содержит только operation hash, ограниченные
file metadata/numeric statuses, control markers, pid/tid и разрешённые kernel
symbols. Cmdline, env, kernel addresses, raw journal/reason/commands не читаются.
`stack.state=3` допустим при запрете доступа procfs; это не причина timeout.
Отдельно сохранить immutable source/SDK SHA, runner exit и cleanup receipt.

Ровно один owned временный Compose project через Base ComposeHelper v2, 30 GiB
preflight и finally cleanup собственных ресурсов. Disposable target/cache/tmp
не смешивать с evidence. Постоянные runtime, snapshots, volumes и images не менять.
Этот source packet не утверждает Cargo/PG PASS или live acceptance.
