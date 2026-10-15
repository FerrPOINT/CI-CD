# Быстрые повторные проверки и сборка

Применить [стандарт Base](https://github.com/FerrPOINT/services-base/blob/68cb8b930b0543a7ecb7f8a34c347b96fd0cd8ba/docs/BUILD_PERFORMANCE.md) без изменения runtime и SDK/UI/skills pins.
Product-owned команды описаны в .quality-gate.json. Сохранять полный обязательный
CI/DB/security/compatibility набор на интегрированной вехе. Во время реализации
scoped tests, reuse только deterministic exact-candidate evidence.
Проверить no-change, один компонент, dependency/toolchain change и corrupt output;
измерить duration и cache hit/miss. Сохранять прогретые caches и чистить own scratch.
