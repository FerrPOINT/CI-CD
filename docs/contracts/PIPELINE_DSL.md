# Pipeline DSL

**Статус:** Target approved. Нормативный контракт `.forge-ci.yml`; текущее runtime-поведение приведено только для миграции. Канонические правила контрактов закреплены [ADR-0009](../adr/0009-canonical-registry.md).

## 1. Источник и модель

Forge читает `.forge-ci.yml` только из полного immutable `resolved_sha`, а не из перемещаемой ветки. До создания executable job сохраняются raw YAML, SHA-256, semantic `version`, точная `parserVersion`, normalised plan и plan hash. Одинаковые `(resolved_sha, YAML, parserVersion, policy snapshot)` обязаны давать одинаковый plan hash.

Конфигурация описывает intent, а не executor privileges. Она не может назначать host mount, Docker socket, privileged mode, host network, service account Kubernetes, произвольный plugin или server-side shell interpolation. `commands` передаются executor-у как массив строк; Forge не интерпретирует их как YAML-template.

## 2. Состояние поддержки

| Область | Current verified | Target approved |
|---|---|---|
| Формат | unversioned `stages`/`jobs` | обязательный `version: 1`, job-level DAG |
| Job | `name`, optional `image`, один `command` | mapping `jobs.<key>` с `commands`, `needs`, policy и metadata |
| Порядок | порядок stages; snapshot хранит legacy-linear dependency edges между стадиями; jobs одной стадии параллельны | `needs` определяет DAG; stages не задают зависимости |
| Trigger | pipeline создаётся manual или текущим Git push hook | `on.push`, `on.pull_request`, `on.schedule` |
| Parser | `serde_yaml`, durable `pipeline_plans` snapshot с parser version `forge-legacy-linear/1`, config/plan SHA-256 | safe parser, diagnostics, full v1 immutable plan |
| Fallback | если локальный config не прочитан, сохраняется `legacy_template` source и template YAML snapshot | только явный `legacy_template` source для migration/demo |

Текущий parser принимает только следующий legacy shape и не принимает `version`, `on`, `defaults`, top-level `jobs`, `needs`, secrets или artifacts:

```yaml
stages:
  - name: build
    jobs:
      - name: compile
        image: rust:1.86
        command: cargo build --release
```

Пустые stages отбрасываются; после этого нужен хотя бы один job. `image` по умолчанию `alpine:3.21`. Текущий режим сохраняет immutable `legacy-linear` snapshot, но остаётся compatibility behaviour, не реализацией target DSL.

## 3. Грамматика target v1

Имена YAML key чувствительны к регистру. YAML document должен быть object; duplicate keys, aliases/anchors, custom tags и неизвестные ключи запрещены.

```ebnf
file          = "version" ":" 1, [ "on" ":" triggers ], [ "defaults" ":" defaults ], "jobs" ":" jobs ;
triggers      = trigger-list | trigger-map ;
trigger-list  = "[" trigger, { "," trigger }, "]" ;
trigger-map   = trigger-entry, { trigger-entry } ;
trigger-entry = "push" ":" trigger-filter | "pull_request" ":" trigger-filter | "schedule" ":" schedule-filter ;
trigger       = "push" | "pull_request" | "schedule" ;
defaults      = [ "image" ":" image ], [ "timeout" ":" duration ], [ "tags" ":" strings ], [ "retry" ":" retry ] ;
jobs          = job-key ":" job, { job-key ":" job } ;
job           = [ "needs" ":" strings ], [ "image" ":" image ], "commands" ":" strings,
                [ "tags" ":" strings ], [ "timeout" ":" duration ], [ "retry" ":" retry ],
                [ "artifacts" ":" artifacts ], [ "secrets" ":" secret-names ], [ "allow_failure" ":" boolean ] ;
retry         = "max_attempts" ":" integer, [ "retry_on" ":" retry-causes ], [ "backoff" ":" "exponential" ], [ "max_backoff" ":" duration ] ;
artifacts     = "paths" ":" paths, [ "expire_in" ":" duration ] ;
```

`job-key` соответствует `^[a-zA-Z][a-zA-Z0-9_.-]{0,62}$` и уникален. `image` - allowlisted OCI image reference; production policy может требовать `@sha256:` digest. `duration` использует положительное целое с единицей `s`, `m`, `h` или `d`. `secret-names` соответствуют `^[A-Z][A-Z0-9_]{0,127}$`. `paths` относительны workspace, не содержат `..`, NUL или symlink escape.

Минимальный target-конфиг:

```yaml
version: 1
on: [push]
defaults:
  image: alpine:3.21
  timeout: 20m
jobs:
  build:
    commands: ["cargo build --release"]
  test:
    needs: [build]
    image: rust:1.86
    commands: ["cargo test"]
```

## 4. Ключи v1

| Расположение | Ключ | Семантика |
|---|---|---|
| root | `version` | обязательное semantic DSL version; сейчас только integer `1` |
| root | `on` | future trigger filter; не меняет ручной запуск |
| root | `defaults` | defaults для jobs: `image`, `timeout`, `tags`, `retry` |
| root | `jobs` | обязательный mapping immutable job definitions |
| `jobs.*` | `needs` | список predecessor job keys; все должны успешно завершиться, если policy не разрешает failure |
| `jobs.*` | `image`, `commands`, `tags`, `timeout` | execution specification и placement requirements |
| `jobs.*` | `retry` | `max_attempts`, `retry_on`, optional exponential backoff |
| `jobs.*` | `artifacts` | declared `paths` и optional `expire_in`; runner собирает только их |
| `jobs.*` | `secrets` | объявленные secret names; values выдаются runner-у отдельным protocol endpoint |
| `jobs.*` | `allow_failure` | optional boolean; default `false` |

Job наследует scalar/object defaults только если ключ отсутствует; list `tags` не объединяется, а заменяет defaults. `commands` обязателен после inheritance, содержит 1..64 непустых строк и выполняется в порядке списка. `needs: []` означает source node. Цикл, self-dependency, ссылка на отсутствующий job или дублирующий key - planning error без queue row.

`retry.max_attempts` от 1 до 5. Default: `1`; для deploy/protected pools default и maximum могут быть дополнительно уменьшены policy. Допустимые `retry_on`: `runner_lost`, `infrastructure`, `timeout`; project exit code, parser/policy error, cancellation и stale lease не повторяются без отдельной policy.

## 5. Future triggers

`on` объявляет, когда server-side trigger policy может создать pipeline. Он не передаёт события напрямую runner-у и не заменяет RBAC/project policy.

```yaml
on:
  push:
    branches: [main, release/*]
  pull_request:
    branches: [main]
  schedule:
    names: [nightly]
```

- `push` фильтрует Git push по target ref; разрешены exact branch/tag names или ограниченный `*` glob.
- `pull_request` фильтрует целевую ветку PR. Pipeline получает immutable source SHA, event id и PR context; доверие к secrets определяется server-side fork/protected-branch policy.
- `schedule` сопоставляет именованный server-side schedule, а не определяет cron. Cron, timezone, DST, missed-run policy и дедупликация принадлежат scheduler contract.
- `on: [push, pull_request]` эквивалентен включению обеих trigger entries без filter. Отсутствие `on` означает, что config разрешает только explicit manual/API trigger до принятия project default policy.

Поддержка этих ключей включается только после реализации соответствующего ingress/scheduler и feature flag. До этого parser либо отвергает config как unsupported, либо использует legacy parser только при явно выбранном legacy source; он никогда не должен silently ignore trigger condition.

## 6. Parser version и совместимость

`version` описывает semantic DSL, а `parserVersion` - конкретную реализацию parser-а (например, `forge-dsl/1.0.0`). Pipeline snapshot обязан хранить оба значения. Re-run использует сохранённый normalised plan, а не заново читает branch или новейший parser.

- Parser принимает только известные semantic versions; unknown `version` является diagnostics `unsupported_dsl_version`.
- Patch parser releases могут исправлять diagnostics/implementation, но не меняют normalised semantics уже сохранённого plan.
- Добавление ключа, изменение default, типа, precedence, trigger или state semantics требует новую semantic version либо explicit migration rule; старый parser отвергает неизвестный ключ.
- Обратная совместимость реализуется отдельным parser adapter-ом, не неявным преобразованием пользовательского YAML. Поддерживаемые versions и срок удаления объявляются release policy.
- Parser выдаёт line/column diagnostic без secret values; invalid config создаёт `failed_planning`/`invalid` pipeline и никогда не enqueue-ит job.

## 7. Лимиты и validation

| Ограничение target v1 | Максимум |
|---|---:|
| YAML bytes | 1 MiB |
| nesting depth | 32 |
| jobs до matrix expansion | 500 |
| `needs` на job / всего DAG edges | 64 / 10 000 |
| commands на job / UTF-8 bytes одной команды | 64 / 16 KiB |
| tags и secrets на job | 64 / 64 |
| artifact paths на job | 32 |
| timeout | 24h |
| expanded jobs | 2 000 |

Parser обязан применять size/depth limits до materialisation, validate image/tag/secret/artifact allowlists и проверять, что project policy допускает secret, runner pool и retention. Matrix syntax не поддерживается в v1, поэтому expanded jobs равны declared jobs; лимит заранее резервирует совместимый будущий extension. Любой limit/policy violation - diagnostic, а не truncation.

## 8. Fallback template

`legacy_template` - отдельный server-side `config_source`, разрешённый только migration/demo project policy. Он создаёт фиксированные linear stages `build`, `test`, `deploy` с legacy jobs; template snapshot/hash сохраняется вместе с pipeline. Пользовательский YAML не может включить fallback ключом.

При `config_source=repository` отсутствие, unreadability или invalid `.forge-ci.yml` является planning error. Forge не подменяет её template и особенно не запускает deploy. Существующее current behaviour, где отсутствующий local config использует template, сохраняется только до migration за явным compatibility flag и должно быть наблюдаемым в pipeline metadata.
