# CI/CD project/admin recovery

Проверено 2026-09-23 на production frontend bundle по штатному адресу
`http://localhost:7712` с настоящей сессией Central Auth.

## Live check

- Авторизованный запрос приложения `GET /api/v1/projects` вернул HTTP 200.
- Рабочий каталог содержал 0 проектов.
- Рабочие проекты, пользователи и раннеры не создавались и не изменялись.

## Safe fixtures

Read-only route fixtures дали 25 проектов, по одному pipeline на проект и
четыре runner-а. Один project-runs запрос намеренно отвечал 503. POST, PATCH и
DELETE проекта намеренно отвечали 500, поэтому recovery мутаций проверен без
записи в рабочую базу.

Подтверждено:

- partial dashboard сохраняет восемь доступных runs и восстанавливает
  недоступный источник локальным retry;
- runner status восстанавливается отдельно от остального dashboard;
- initial projects error локализован и восстанавливается без reload страницы;
- каталог показывает 12 строк на странице, поиск отличает no-match от empty;
- failed create/edit сохраняют draft, failed delete сохраняет dialog;
- settings search поддерживает filter, no-match и clear;
- users ведёт в центральный Admin Panel;
- mobile drawer удерживает фокус, закрывается по Escape и возвращает фокус;
- login и SSO callback error доступны во всех темах и viewport.

## Browser matrix

Проверены шесть маршрутов (`/`, `/projects`, `/settings`, `/users`, `/login`,
`/sso/callback`) на ширинах 375, 768, 1280, 1920 и 2560 px в темах light,
gray и dark: всего 90 состояний.

- document horizontal overflow: 0;
- видимые interactive targets меньше 40x40 px: 0;
- unnamed controls: 0;
- неожиданные nested scrollers: 0;
- serious/critical Axe violations: 0;
- неожиданные browser runtime и transport errors: 0.

`qa-report.json` содержит полную матрицу, workflow assertions и перечень
намеренных 500/503. PNG фиксируют representative mobile, desktop и wide states.
