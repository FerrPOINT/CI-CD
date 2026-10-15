# Быстрые повторные проверки и сборка

Применить [стандарт Base](https://github.com/FerrPOINT/services-base/blob/ad724088e9a63b1e353fb5b28e4308e9ae6f45ae/docs/BUILD_PERFORMANCE.md) без изменения runtime и SDK/UI/skills pins.
Product-owned команды описаны в .quality-gate.json. При разработке/сборке —
unit/static; integration/slow/full coverage — вручную перед релизом.
Security/data/compatibility/live gates сохранены; reuse только для допустимых
deterministic exact-candidate проверок.
Проверить no-change, один компонент, dependency/toolchain change и corrupt output;
измерить duration и cache hit/miss. Сохранять прогретые caches и чистить own scratch.

Docker frontend: раздельные unit/compile stages, проверка unit перед compilation;
production COPY исключает тесты. Frozen dependencies сохраняют общий совместимый
pnpm download store. Dockerfile syntax 1.19 и parent digests закреплены.
Runtime и package build scripts сохраняют прежние публичные команды.
