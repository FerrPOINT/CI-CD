# Локальная интеграционная проверка workspace

Этот fixture проверяет настоящий Forge API, PostgreSQL, worker, CLI и Base
operator на disposable Compose окружении. Установка Forge на общем сервере для
него не нужна. Проверяются регистрация, задача/сессия, один writer, busy Stop,
повтор исходной команды, конфликт изменённого payload, Stop/Start, данные и
container/image/mount identity. Tracker здесь — синтетический HTTP fixture.
Результат не подтверждает права действующего Tracker или Kubernetes/R1 runtime.

Используются уже собранные immutable images. Driver не делает build/pull,
работает с уникальными Compose project/volumes и штатной custody Base.
PostgreSQL предназначен только для тестового Forge. Приложение fixture хранит
маркер на своём volume; R1 и соседние БД/volumes не подключаются.
Docker socket нужен доверенному operator для управления этим disposable target.
Тест не предназначен для запуска внутри пользовательского job.

## Запуск

Нужны Docker Compose, принятые Base tooling, готовый Forge runtime с workspace
operator/CLI, его квитанция и локальный PostgreSQL 17 image. Квитанция runtime
содержит `image` и `candidate` выбранного состава; driver также проверяет
встроенный `/opt/forge-runtime/SHA256SUMS`. Перед прогоном сверить её source/image
с исходными release evidence. Неизменные проверенные images не пересобирать.

Из корня Forge в PowerShell:

```powershell
& $ToolPython "$Base/scripts/execution_slot.py" -- $ToolPython `
  scripts/workspace_fixture/qualify.py `
  --base-tooling $Base --docker-context desktop-linux `
  --runtime-receipt $RuntimeReceipt --postgres-image $PostgresImage `
  --output $PrivateEvidenceDirectory
```

`$ToolPython` — tools Python, `$Base` — принятый checkout Base с Compose helpers.
На Linux используйте context своей проверенной Docker installation. Образы
должны уже находиться в этом daemon. Если host execution slot занят другой
сборкой/квалификацией, не запускайте второй тяжёлый процесс. До запуска также
проверяются внешние builders, которые могут не удерживать общий slot.

Результат сохраняется в `<output>/<уникальный project>/result.json`.
Raw controller logs и synthetic identity files остаются в private output.
Fixture не меняет installation resource profile и не требует квот production.

## Windows helper R1

Дополнительно можно пройти совместимый `dev.ps1` и настоящий Windows CLI:

```powershell
# Добавить к предыдущей команде:
--r1-source $R1Checkout --windows-cli-receipt $WindowsCliProducerReceipt `
--test-python $WindowsToolsPython
```

В `$R1Checkout` должен быть `scripts/pdlc/test_workspace_integration.py`.
Квалифицированный `cicd-cli.exe` находится рядом со своим `producer.json`,
содержащим `sha256`; tools Python имеет pip для установленного Windows launcher
и pwsh в PATH. Никакой CLI compilation/installation во время теста нет.

Driver публикует только loopback ports и передаёт синтетический fixture JSON в
`R1_WORKSPACE_INTEGRATION_FIXTURE`. Тест адаптирует только transport URL и Auth
fixture, сохраняя product origin guard; HTTP запросы/команды исполняет настоящий
Forge/CLI. Реальные production credentials и DNS не используются. После успешного
POST тест намеренно теряет ответ, затем читает и повторяет исходную операцию,
проверяет запрет изменённых аргументов/второго writer/busy Stop и сохранение
данных после полного Stop/Start.

Без явно выбранного fixture этот integration test помечен `skipped`, а не
успешным native gate. Обычные tooling unit tests не требуют Docker. Реальная
приёмка R1, Kubernetes RBAC, delivery и browser/data evidence остаются отдельными.
