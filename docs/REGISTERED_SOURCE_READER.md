# Проверка источника registered reader

Текущий route split — source candidate без нового Rust/PG/HTTP PASS;
runtime не изменён. Typed routes и общий wiring интегрированы в source;
итоговую qualification и генерацию OpenAPI выполняет Main.

Прежний `GET /api/v1/namespace-repositories/{id}` сохраняет reader auth и обычный
catalog JSON с `kind`. Отдельный `GET /api/v1/namespace-repositories/{id}/source`
с тем же registered-reader auth проверяет обязательные query-поля `base_ref` и
`base_commit`: полная `refs/heads/...`, точный SHA1 из 40 lowercase hex. Частичная
пара, короткое имя, tag, HEAD/revision expression и caller proof отклоняются.
Новых scopes, credentials, registry, таблиц или миграций нет. Старый endpoint
никогда не возвращает source DTO вместо своего catalog object.

Ответ `VerifiedSource`: namespace, Forge instance/repository UUID, public name,
base ref/commit, default ref, protected refs и время наблюдения. Это те же девять
полей, что ожидает Tracker. Catalog UUID определяет hosted ready storage, а не
имя из запроса. Существующий catalog/namespace admission удерживается до конца
native readback. Source SHA берётся из точного PR branch snapshot и проверяется
как настоящий commit. Default читается из symbolic HEAD; detached/неизвестный
default не подменяется `main`. Guard включает native default и ВСЕ default/
protected branches связанных проектов, в том числе ещё не созданные ветки.
Некорректная/слишком большая policy закрывает ответ, а не усекается.

Операция не создаёт ветки, hooks, pipeline, PM run или Git grant и не меняет
исходную задачу/assignment. Прежние receive guards остаются авторитетными при
каждом push. Историческое наблюдение не заменяет current Developer/run/fence/
lease после owner confirmation. Tracker HTTP adapter поставляется отдельно;
native/DB/auth gates выполняет Main в существующей QA, без новых fixtures как live.
