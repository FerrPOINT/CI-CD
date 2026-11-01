# Оптимизация сборок текущего Forge без upstream runtime adoption

Исходный Source — actual PDLC2 `5ab203dd`; modern performance candidate `42c7f6d0`
сохранён отдельно, потому что содержит дополнительный Kubernetes/runtime cohort.
Работа ведётся в существующем task worktree, без нового clone/export и без
переключения actual main/runtime. SDK/UI/skills/toolchain pins сохраняются.

## Исполнение

1. Подключить Base descriptor и portable selector к реальным Cargo targets и
   существующим service names. No-change/docs/test-only не запускают app producers;
   неизвестный путь расширяет соответствующий продукт. Base ref валидируется до подготовки.
2. Automatic CI выбирает компоненты, выполняет быстрые unit/static checks и selected backend cargo check;
   frontend dist создаётся только при изменении production inputs. PostgreSQL/full workspace/native/security/compatibility gates
   прежнего CI сохраняются в отдельном ручном release.yml -> qualification.yml; не запускать их
   после каждого изменения. Не готовить PostgreSQL для domain-only unit graph.
3. Сократить Docker compiler graph до поставляемых cicd-server/forge-runner; CLI,
   delivery, pg-migrate и migration сохраняют явные отдельные команды. Не переносить
   отсутствующий forge-quiesce или runtime tools нового upstream. Сохранить реальные
   embedded-helper/TLS/migration consumers, dependency layers и свежесть Rust inputs.
4. Проверить Source selection, ordinary helpers и retained manual gate inventory
   быстрыми адресными controls. Native builds/CPU/RAM/old timestamps/invalidation
   проверяются одной интегрированной вехой через общий host slot; сейчас их не запускать.

## Доказательства и границы

До переноса actual Docker cook собирал default package binaries, final RUN также
компилировал cicd-cli; образ копировал cicd-server/forge-runner. Automatic backend CI
готовил PostgreSQL/CLI DB и выполнял all-targets clippy/workspace build независимо
от выбранного компонента. Это Source observations, не измеренный compiler speedup.

После каждого закрытого пункта фиксировать исчезнувшую работу, штатную команду и
scoped proof. App/resource/native/runtime acceptance не выдавать за metadata PASS.
Канонические правила: Base docs/BUILD_PERFORMANCE.md и docs/PERFORMANCE_SPEC.md
в принятых quality tooling; общий план — Base docs/plans/build-optimization-delivery-plan-20261017.md.

## Ручной workflow без дублирования

`release.yml` содержит canonical workflow_dispatch и вызывает same-commit
`qualification.yml`; полный старый CI хранится только в reusable body. Direct
workflow_dispatch сохраняет git_receive_only, release wrapper передаёт false.
Caller не объявляет второй concurrency owner. Токен Base передаётся только
именованным secret. При reusable call github context относится к caller, поэтому
старые workflow_dispatch conditions сохраняют смысл при ручном release запуске.
Проверено по [reuse workflows](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)
и [context/concurrency contract](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations)
1 ноября2026. Hosted execution ещё не проводился.

## Scoped terminal, без native сборок

Исходные адресные Python-проверки: 29 PASS / 4 SKIP; CSS input fixtures: 2 PASS
на Node 22.22.2 с read-only использованием совместимых установленных пакетов.
Public `scripts/build.py` проверен отдельными controls с настоящим Git fixture
и inert внешними endpoints: clean/docs/test-only не вызывают ни verifier, ни
producer; backend/frontend выбираются раздельно, все состояния Git учитываются,
runner сохраняет явный/profile путь, смешанные pins отказывают до подготовки.
Это unit-доказательство маршрутизации, не Docker/native qualification.

Manual CI сохраняет прежние full gates; Cargo cache содержит только скачанные
зависимости. Automatic и manual используют один dependency key без selected-gate
флагов: другой scope не требует второй копии тех же скачиваний. Compiled target
не сохраняется между независимыми Source exports. Длительность native сборки,
CPU/RAM и old-timestamp регрессия остаются NOT_MEASURED/NOT_RUN.
