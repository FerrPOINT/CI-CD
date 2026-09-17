# Dependabot Audit — 2026-09-17

Scope: open Dependabot pull requests in Base repositories. This audit is read-only: no PR was merged, closed, rebased, or modified.

| Repository | PRs | Evidence | Recommendation |
|---|---:|---|---|
| services-base | #21 | `lucide-react` major update conflicts with current `main`, though existing CI is green | Recreate as a fresh, isolated upgrade branch; do not resolve/merge the stale PR directly. |
| services-base | #22 | `eslint-plugin-react-refresh` minor; mergeable; backend/frontend checks green | Safe candidate for a dedicated dependency batch after explicit approval. |
| services-base | #23 | `vitest` 5 major; mergeable; checks green | Keep separate from production dependency updates; run frontend suite and inspect test-runner migration notes before approval. |
| services-base | #24 | `typescript-eslint` minor conflicts with current `main`, though checks green | Recreate from current `main`; bundle only with compatible lint tooling after explicit approval. |
| java-agent | #31, #33, #35 | Groovy patch updates in the Telegram module; all JDK 25, Markdown/YAML, Docker checks green; GitHub still reports mergeability unknown | Combine into one fresh Gradle patch batch after explicit approval; validate full module suite. |
| java-agent | #34, #36 | LangChain4j 1.18 to 1.20 production API update; checks green but semantic behavior is high impact | Do not merge automatically. Create a dedicated compatibility review with tool-calling, streaming, and provider regression coverage. |
| java-agent | #37 | JLine minor update; checks green; mergeability unknown | Safe candidate, preferably batched with the other low-risk Java patch/minor updates after explicit approval. |
| java-agent | #38 | Spring Boot 4.1.0 to 4.1.1 patch; checks green; runtime framework dependency | Candidate after the current runtime release stabilizes; require full Java suite and container smoke after explicit approval. |
| java-agent | #39 | jsoup 1.18 to 1.23 minor production parser update; checks green | Treat as parser/security compatibility batch; test HTML extraction and SSRF boundaries before approval. |
| admin-panel | #1 | Rust build image 1.88 to 1.98; all checks green; substantial toolchain leap | Do not merge automatically. Needs MSRV/edition, Cargo lock, build-cache, and runtime image compatibility review. |
| admin-panel | #2 | nginx 1.29 to 1.31 image update; all checks green | Safe candidate after reviewing release/security notes; retain a standalone image-update commit. |
| admin-panel | #3 | Node 22 to 26 image update; all checks green; major runtime leap | Do not merge automatically. Requires pnpm/Vite production build and browser smoke on Node 26.

## Decision

No outstanding PR is safe to merge without user authorization. The next execution batch should be one explicit, low-risk dependency approval group (`services-base #22`; Java #31/#33/#35/#37/#38 subject to current mergeability) followed by isolated compatibility work for LangChain4j, jsoup, Vitest, Rust, and Node major updates.
