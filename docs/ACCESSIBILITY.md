# Accessibility — Forge CI/CD

> **Целевой стандарт:** WCAG 2.2 AA. **Статус:** частично Current verified; программная верификация Target approved. Скриншот не является доказательством доступности.

## 1. Scope

Требования распространяются на Dashboard, Git/CI/CD экраны, администрирование, формы, диалоги и mobile viewport 375 px. Требования применяются к новой UI-работе до merge, а не постфактум перед релизом.

## 2. Что проверено сейчас

| Мера | Evidence | Статус |
|---|---|---|
| Mobile overlay drawer: кнопка имеет `aria-label`, Escape закрывает drawer, focus возвращается trigger | `frontend/src/widgets/app-shell.tsx` | Current verified |
| Confirm dialogs используют Radix AlertDialog (focus trap, semantic roles), не `window.confirm` | `frontend/src/shared/ui/confirm-dialog.tsx` | Current verified |
| Runners/users используют cards на mobile вместо сжатых таблиц | `frontend/src/pages/runners/index.tsx`, `users/index.tsx` | Current verified |
| Таблицы имеют `caption`/`sr-only` и column headers там, где реализовано | `runners/index.tsx` | Current verified |
| Статусы не передаются только цветом: есть текст `Онлайн`, `Успешно`, `Открыт` | UI + `m-runners.png` | Current verified |
| 375 px evidence для ключевых flows | `docs/assets/screens/manifest.md` | Current verified |

`min-h-9` (36 px) — текущий минимальный размер компактного элемента. Для primary mobile actions целевая норма — 44×44 CSS px; 36 px допускается только для вторичных dense-table действий с явным aria-label.

## 3. Известные пробелы

- Нет автоматического axe-core теста и нет Lighthouse accessibility CI gate.
- Контраст palette dark/light/gray не измерен программно; соответствие AA не заявляется.
- Нет полного keyboard-only прохода по всем маршрутам и form validation/error announcements audit.
- `aria-live` политика для async mutations/toasts не задокументирована.
- Скриншоты доказывают responsive layout, но не screen-reader semantics, focus order или contrast.

## 4. Программа верификации (Target approved)

| Gate | Критерий | Evidence |
|---|---|---|
| axe in Playwright | 0 serious/critical violations на authenticated/public representative pages | `frontend/e2e/accessibility.spec.ts` report |
| Lighthouse | Accessibility score ≥95 на desktop и 375px mobile | CI artifact JSON/HTML |
| Keyboard journey | Tab/Shift+Tab/Escape/Enter/Space работает: nav, drawer, create forms, dialogs, destructive action | Playwright scenario + manual checklist |
| Contrast | text, controls, focus indicator проходят WCAG AA; non-text UI ≥3:1 | palette audit report |
| Responsive | 375, 768, 1920 px без horizontal clip у critical flows | screenshots manifest + visual review |

## 5. Правила для контрибьюторов

1. Интерактивный элемент имеет понятное доступное имя: видимый текст либо `aria-label`; icon-only action всегда `aria-label`.
2. Не удалять focus outline. Custom focus style должен быть заметен на всех темах.
3. Цвет не является единственным носителем значения: рядом размещается текст/icon/shape.
4. Primary touch target на mobile ≥44×44 px; compact secondary control ≥36×36 px только с достаточным spacing и accessible name.
5. Формы имеют связанный `label`, required/error state и понятный текст ошибки; placeholder не заменяет label.
6. Dialog/drawer обязаны иметь focus management, Escape close и восстановление focus; использовать Radix primitives вместо самодельного overlay.
7. Новый маршрут или интерактивный flow добавляет a11y-case в Playwright после появления target harness и обновляет `TRACEABILITY.md` при затрагивании REQ-UI/NFR.

## 6. Evidence ownership

Frontend owner поддерживает этот документ и a11y-gates. Изменение design tokens, интерактивных primitives или navigation требует review этого файла. Серьёзное axe finding — SEV2 по `TEST_PLAN.md` до доказательства обратного.
