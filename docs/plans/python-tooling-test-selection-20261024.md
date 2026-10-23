# Точный выбор Python helper tests

Реализация утверждённого плана PDLC: известные helper test paths не должны
выбирать application producers через unknown fallback.

1. Проверить исполнение и полный source-input closure каждого модуля.
2. Добавить regression через фактический packaged selector: Fast/Unit/Build,
   неизвестный script, соседний application input и CI module ownership.
3. Добавить exact ownership и отдельные pure Python gates, не меняя существующие
   artifact/native/manual gates и pins. Согласовать CI без двойных unit modules.
4. Выполнить scoped Python checks и публичный root PlanOnly. Production builds,
   ручные integration/slow/release и runtime для этой source-правки не нужны.

Проверка source-сценария не является native build/resource qualification.

## Результат 24 октября

RED воспроизвёл README timestamp mutation в caller-owned временном fixture.
Оба отрицательных сценария переведены на собственный TemporaryDirectory;
дополнительная regression сохраняет bytes/mtime caller README. Устаревшая
проверка литерала pnpm install заменена проверкой frozen flag при разрешённых
дополнительных options; отсутствие frozen flag и install перед pin дают RED.
Прежние точные build-contract ownership assertions сохранены через reviewed map.

Source run:126cases,124PASS/2POSIX SKIPPED,3,874с с загрузкой. Guard разрешал
только Python/Git children; actual Docker/Rust/Gradle/UV0. README bytes/mtime
сохранены. После финального routing fixture повторены только3routing cases.
Public root Unit/tooling PlanOnly exit0. Linux POSIX cases и explicit SDK suite
здесь NOT RUN; Windows не выдаётся за их proof. Native runtime/resources не
измерялись; application install/export/build0.

Quality/SDK/UI/skills/operator pins и соседние workspace не менялись.
