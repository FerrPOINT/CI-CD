# Минимальный backend graph PDLC3

Область: обе backend Docker recipes, их context policies, обычные source guard,
CI/quality callers. Общие правила — [инструкции по pinned Base standard](../../AGENTS.md).

План: metadata/stubs → unit dependencies → full dependencies → реальные unit
sources и независимый полный build с marker. Unit не наследует server graph.
Убирать product stubs после dependency compilation, не сохранять writable
compiled target между exports; общий architecture-qualified Cargo188 download
cache и корректные Docker layers сохранять. Workers2 — исходный безопасный ARG,
не измеренный fastest profile.

Forge: сохранить собственный root package и поставку двух binaries — server
и forge-runner. CLI/OpenAPI не являются payload этого runtime image; их source
entrypoints и manual CLI integration сохраняются. В этой копии нет cache-protocol
suite, не добавлять чужую suite или зависимость ради сходства recipes.

Контексты ограничить необходимыми source roots, сохраняя private exclusion tail
workspace. Не менять SDK/UI/skills/operator pins, Cargo locks, toolchains,
runtime users/ownership/paths/ports/packages/commands и работающие контейнеры.
Source RED/GREEN, consumer/fixture/metadata graph и штатный Fast tooling/docs —
проверки этого этапа. Native old-timestamp/invalidation/cache-loss/units,
correct artifacts/runtime и CPU/RAM/wall/context bytes нового tuple пока NOT_RUN
до admission на свободном хосте; manual gates сохраняются отдельно.
