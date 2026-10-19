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
