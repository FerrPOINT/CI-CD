# Owner selection source qualification, 2026-10-19

The candidate extends merged namespace-reader evidence with explicit privileged owner selection. Inventory does not authorize first deployment; missing, duplicate, stale, foreign or ambiguously encoded artifacts fail closed. No new endpoint, credentials, migration or installed enrollment is introduced.

Local Firebat2 qualification of backend tree at 57bbfeb335d8087bf67e88181c015bebae9d4bae used Rust 1.88, locked offline dependencies and one worker:

- 376 server-library tests passed; 6 ignored tests were not counted.
- Four PostgreSQL cases ran explicitly: two authenticated namespace-reader cases, one exact owner-selector case and one production artifact-observer SQL case. All passed on a disposable database.
- Five artifact-name domain tests passed. Rust fmt and strict release Clippy for the changed server library/tests passed.
- OpenAPI was exported by the actual release binary. All paths and existing schemas remain equal except the additive optional `TargetEvidence.selection`; two schemas describe its closed contract. The YAML SHA256 is `427f898fa161caeba0e817ec7009267155bcb05ab586565cedaf60af3c861061`.
- Frontend types were generated from this export with the pinned tool, then TypeScript and OpenAPI drift checks passed using the existing frozen installation. No UI implementation changed, so the previous UI build is reused.
- CI now explicitly runs both newly ignored PostgreSQL cases with nonzero exact-count guards. Source-only checks from the earlier publication remain separate from these actual Rust/PostgreSQL results.

Useful compiler cache is retained. Temporary fixture resources were removed. Enrollment, real deployment, authenticated runtime acceptance and production updates were not performed. Hosted CI is recorded separately.

## Focused Local Gate

Отдельный локальный gate на exact source `57bbfeb335d8087bf67e88181c015bebae9d4bae`
и SDK2ad: **13/13 PASS**, 9 unit, два connection-private SQL-projection PG и два
real migrated R HTTP/PG. Positive/negative R cases проходят 3.15s/2.20s с одним
PG connection, exact retained custody/selection и no-write assertions.
Fmt, typed affected libs/bins, strict affected Clippy и actual Rust OpenAPI export
проходят. Test profile opt0; production release/server/image этим gate не собирались.
Runner exit0, ComposeV2 и disposable inputs cleanup подтверждены; собственные
containers/networks/volumes отсутствуют. Старые failed receipts сохранены отдельно.
Result SHA256 `634e043260249385c286a3d531b8c25aafab3d1949b3e9877b5105de91d99c63`.

Committed YAML побайтно совпадает с actual export SHA256
`427f898fa161caeba0e817ec7009267155bcb05ab586565cedaf60af3c861061`.
Existing `openapi:generate` на Node22.20/pnpm10.28.1/openapi-typescript7.13 создал
клиент без ручных изменений. Host `openapi:check`, compatibility against ab2,
standalone strict schema TypeScript и explicit schema ESLint проходят.
Unique-key YAML comparison подтверждает прежние paths, 189 unique operation IDs,
только optional `TargetEvidence.selection` и две новые selection schemas.
Это scoped generated-contract validation, не повтор полного frontend gate.
Source backend/locks/pins не изменены относительно квалифицированного 57bb.
