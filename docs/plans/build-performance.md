# Быстрые повторные проверки и сборка

Применить [стандарт Base](https://github.com/FerrPOINT/services-base/blob/c9e4182e460ee57ac38a0ab1a866ce9626d297a4/docs/BUILD_PERFORMANCE.md) без изменения runtime и SDK/UI/skills pins.
Product-owned команды описаны в .quality-gate.json. Сохранять полный обязательный
CI/DB/security/compatibility набор на интегрированной вехе. Во время реализации
scoped tests, reuse только deterministic exact-candidate evidence.
Проверить no-change, один компонент, dependency/toolchain change и corrupt output;
измерить duration и cache hit/miss. Сохранять прогретые caches и чистить own scratch.
