# Совместимый CI selector для frontend reuse

Local root уже использует exact quality0d8d59bd, но product CI selector
оставался на339c1b0c и отвергал существующий frontend-agent-presence-v1.
JSON-only scope test не вызывал настоящий loader и пропустил расхождение.

Штатный Base packager обновляет только product quality pin и selector до
того же0d8d59bd. Все установленные optional packet features сохраняются;
Rust cache/frontend artifact helper и runtime recipe bytes не меняются.
Root quality/SDK/UI/skills/operator pins не обновляются. Gate checks вызывают
actual vendored loader; old committed selector должен воспроизвести refusal.

Проверки: exact package --check, source tooling/CI checks, committed
vendor/receipt hash equality и scoped plans. Это исправление CI selection,
не новая квалификация frontend units/build текущего commit. Готовый dist и
старые proofs сохраняются; application producer не требуется.
