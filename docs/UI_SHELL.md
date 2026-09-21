# UI shell — Forge CI/CD

Статус: Current verified после browser QA. Область: авторизованные маршруты Dashboard.

## Геометрия

| Viewport | Навигация | Рабочая область |
|---|---|---|
| `<768 px` | Radix drawer до 320 px | полная ширина, header 60 px |
| `768–1279 px` | rail 72 px | отступ слева 72 px, header 60 px |
| `>=1280 px` | sidebar 264 px | отступ слева 264 px, header 60 px |

`main` не задаёт локальный `max-width`: шириной содержимого управляет конкретный экран. Sidebar фиксирован, а header остаётся sticky внутри рабочей области.

## Навигация и доступность

- `NavLink` назначает `aria-current="page"` и сохраняет активность раздела на вложенных маршрутах.
- В rail подписи визуально скрыты, но остаются доступными assistive technology; ссылки имеют `title`.
- Mobile drawer построен на Radix Dialog: фокус удерживается внутри, Escape закрывает drawer, после закрытия фокус возвращается кнопке меню.
- Переход по ссылке закрывает drawer.
- Основные header controls не меньше 40×40 px, ссылки mobile navigation — не меньше 44 px.

## Header

Header содержит переход между сервисами, переключатель темы, идентификатор текущего пользователя и выход. На узком экране вторичные подписи скрываются, но доступные имена элементов управления сохраняются.

## Проверка изменений

Обязательный минимум: unit-тест `frontend/src/widgets/app-shell.test.tsx`, Playwright keyboard scenario в `frontend/e2e/critical-flows.spec.ts`, responsive browser QA на 375/768/1440/2560 px в light и dark themes.
