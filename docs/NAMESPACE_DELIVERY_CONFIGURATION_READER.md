# Namespace delivery-configuration readback

Source-only additive расширение существующего registered-reader GET. Нет нового
endpoint, issuer, scope, controller, migration или исключения для human catalog.
Rust/PG execution, установка и live credential publication не заявлены.

## Exact request

```text
GET /api/v1/namespace-repositories/{repository_id}?registry_instance_id={registry_id}&namespace_id={namespace_id}&base_ref={encoded_full_branch}&base_commit={exact_sha1}&delivery_configuration_name={encoded_exact_name}
```

`delivery_configuration_name` optional. Если указан, требуется непустое имя,
не более 256 UTF-8 bytes, без окружающих пробелов и control characters, вместе с
обоими прежними exact source fields. Это equality selector, не search/pattern,
произвольный SQL или authority от агента. Duplicate query fields и неизвестные
query keys по-прежнему не допускаются.

Существующий `namespace::reader_auth` неизменен: fresh central identity,
subject из `CICD_NAMESPACE__READER_SUBJECTS`, без human session/task grant,
со scope `ci-cd:read`. Local SAT и human/admin fallback не добавляются.
Регистрация subject и credentials остаются отдельными operator-owned inputs;
этот source change их не выдаёт и не публикует. Delegated parent P отличен от
owner O и scheduler S, и не используется вместо отдельного reader R.

## Readback и custody

Без нового selector старые source bytes/порядок полей и catalog variant
не меняются. Новое optional поле отсутствует, а не сериализуется как null.
С selector в source observation добавляется:

```json
{
  "delivery_configuration": {
    "id": "configuration UUID",
    "name": "exact configured name",
    "repository_id": "same verified repository UUID",
    "repository_name": "same verified public group/repository name"
  }
}
```

Прежние source/namespace validation и native Git observation сохранены.
Bounded parameterized projects lookup выполняется в ТОЙ ЖЕ catalog/namespace
transaction до commit, по verified repository UUID и exact name, максимум две
строки. Нет fallback в чужой repository или общий каталог. Missing даёт 404,
ambiguity 409, несовпадение retained identity 403. URLs, storage paths,
tenant/user records, credentials и прочие поля config не выбираются и не
возвращаются. Прежний `Cache-Control: no-store` сохранён.

Последующая Fleet caller integration должна читать этот вложенный объект вместо
human `/delivery-configurations`, сохранив context/repository/source и lease
checks. Reader authority не выводится из delegation parent.

## Тесты и оставшаяся qualification

Четыре новых теста проверяют query bounds, exact projection с missing/duplicate/
foreign rows, byte equality с прежним nine-field source DTO и настоящий reader
middleware с synthetic central Auth/JWKS. Последний проверяет registered PAT
allow, foreign/write-only/task-child/revoked/absent/SAT deny и verified human
admin session deny даже для зарегистрированного subject. Projection использует
synthetic rows. Настоящий namespace handler отдельно проверяется до missing-DB
fail-closed boundary, включая duplicate/empty query rejection.

Это не PostgreSQL integration или native acceptance. Main владеет последующей
Rust qualification, global OpenAPI/schema publication и operator-owned reader
configuration. Default APIs и human auth не изменяются.
