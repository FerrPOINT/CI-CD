# Compare patch limit QA

Проверка завершена 2026-09-23 в Chromium на production-сборке Dashboard.
Использованы реальные Central Auth и platform shell; только read-only ответы
`refs` и `compare` для `qa-compare` заменены детерминированным большим
сравнением. Backend и рабочие данные не изменялись.

Проверено:

- усечённый patch сопровождается явным сообщением о лимите 512 КиБ;
- полный список файлов, статусы, binary marker и line counts остаются доступны;
- patch scroller получает keyboard focus;
- breadcrumb links и команды имеют интерактивную высоту не меньше 40 px;
- ширины `375`, `768`, `1280`, `1920`, `2560` проверены в темах
  `light`, `gray`, `dark`, всего 15 состояний;
- во всех состояниях: 0 horizontal overflow, малых интерактивных целей,
  неожиданных вложенных scroller, runtime/network errors и serious/critical
  axe findings.

Артефакты:

- `compare-375-light.png` — compact mobile layout и truncation notice;
- `compare-1920-gray.png` — desktop compare в серой теме;
- `compare-2560-dark.png` — wide desktop в тёмной теме;
- `qa-report.json` — автоматические проверки матрицы.
