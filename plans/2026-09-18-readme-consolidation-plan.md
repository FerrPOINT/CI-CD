# Forge CI/CD README Consolidation Plan

> **Статус 2026-09-18:** execution plan for the Base README migration. Scope is documentation and validation only; it does not change pipeline semantics, auth, runner behavior or deployment configuration.

## Purpose

Keep the mature Forge visual identity and in-repository screenshot catalogue, while turning the root README into an operator-safe entry point. Detailed target architecture and policy stay in their canonical docs rather than being duplicated in a giant README.

## Verified facts used

- Runtime: Rust 2024/Axum/SQLx/PostgreSQL 17 backend; React 19/Vite 6/Tailwind 4 frontend.
- Base umbrella runtime: backend `7711`, web `7712`; `GET /api/v1/health` and `GET /api/v1/readiness` returned healthy/ready during this migration.
- Repository-local Compose defaults: API `22801`, dashboard `22802`, PostgreSQL loopback `22543`.
- `scripts/verify_docs.py --all` verifies links, anchors, naming/status drift, frontend contracts, screenshots/manifest, plans, OpenAPI and API-route coverage.
- `docs/assets/screens/manifest.md` is the source of truth for 46 screenshots. Existing assets show some synthetic pipeline/repository state; dashboard assets also expose internal seed remotes and must not be added to the public README until redacted.

## Tasks

### 1. Simplify the root entry point

- Replace externally hosted capsule art with a local accessible Forge banner.
- Retain the existing steel/cyan/green visual direction and a short navigation row.
- Preserve verified status vocabulary, but reduce the feature list to capability groups and direct readers to `docs/CURRENT_STATE.md` for exhaustive bounds.

### 2. Correct setup and security guidance

- Use the checked-in `.env.example` and `docs/ENV.md`; never create a key through a shell command that appends a secret to `.env`.
- Distinguish repository-local ports from Base umbrella ports.
- State the conditional auth/trusted-local boundary and that public ingress remains operator-owned.

### 3. Curate public visual proof

- Retain only the blank login screen and synthetic pipeline detail in the root README, plus `m-pipeline-detail.png` as the 375x812 responsive proof.
- Avoid dashboard/repository-browser assets because they contain internal seed topology/remote URLs or repository identifiers.
- Link the complete 46-asset registry instead of duplicating it.

### 4. Extend executable README validation

- Add a dedicated README check to `scripts/verify_docs.py`: explicit anchors, local image paths, no placeholders or local filesystem paths, valid workflow badge files, and required safe visual proof.
- Add unit tests for the check and invoke it from the existing CI docs job via `--all`.

### 5. Verification and release

- Run focused Python tests and `python3 scripts/verify_docs.py --all`.
- Run relevant frontend checks and live Base health/readiness/browser review without modifying runtime data.
- Commit one `docs:` change, fetch/rebase on `origin/main`, push, and wait for the CI run for the pushed SHA.
