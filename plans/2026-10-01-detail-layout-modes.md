# CI/CD: семантика detail-страниц

**Статус 2026-10-01:** Current verified; 187 unit, lint/typecheck/build и no-mock live 2/2 зелёные.

Закрывает оставшуюся CI/CD-часть A04 платформенного UI/UX-аудита.
Backend/API, worker/runner semantics и авторизация не меняются.

| Маршрут | Режим | Основная область | Контекст |
| --- | --- | --- | --- |
| `/pipelines/:id` | `wide` | план, стадии/DAG, job logs | отдельного aside нет |
| `/repositories/:repo/pulls/:number` | `detail-with-aside` | описание и реквизиты PR | действия |
| PR `?view=diff` | `wide` | статистика файлов и patch | отдельного aside нет |
| `/settings` | `reading` | настройки | без изменений |

Обзор PR использует общий `page-split`: fluid primary + 320 px rail от
1024 px, ниже действия следуют после primary в DOM и визуально. Pipeline
сохраняет равные рабочие колонки стадий: их нельзя выдавать за contextual rail.

Проверки: route/query mapping, landmarks/DOM order, полный frontend suite,
OpenAPI/semantic/lint/typecheck/build, docs validators. No-mock production QA
создаёт только собственный внутренний Git-репозиторий и проект, выполняет
безопасные printf jobs, открывает PR и diff; измеряет actual geometry на
375/768/1023/1024/1279/1280/1440/1920/2560 px в трёх темах, axe, keyboard/touch,
console/network и отсутствие UI-мутаций при навигации. Cleanup использует
штатные API только для точных QA project/repository IDs. Рабочий deployment
и пользовательские volumes не затрагиваются. Секреты/trace не публикуются.
Исторические PR/audit rows остаются только в изолированной QA-базе до удаления
собственного Compose-проекта: отдельного DELETE для них нет.

Header ownership и README gallery остаются отдельными пунктами аудита.

Один непрерывный live-прогон на окончательном production image: 108 detail +
84 основных route/theme/viewport сочетания, retries=0, 2,6 минуты. Обе confirm
формы проверены axe, touch/keyboard и actual target height; устранены возврат
фокуса, неверный белый destructive text и desktop min-height override.
[Evidence](../docs/screenshots/2026-10-01-detail-layout/README.md) содержит
full-page кадры и image/config/source fingerprints.
