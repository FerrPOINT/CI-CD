<p align="center">
  <img src="docs/assets/forge-readme-banner.svg" alt="Forge CI/CD - self-hosted Git and pipeline control plane" />
</p>

<p align="center">
  <a href="#capabilities"><img src="https://img.shields.io/badge/Capabilities-0f3a4b?style=for-the-badge" alt="Capabilities" /></a>
  <a href="#quick-start"><img src="https://img.shields.io/badge/Quick_Start-075985?style=for-the-badge" alt="Quick start" /></a>
  <a href="#visual-proof"><img src="https://img.shields.io/badge/Visual_Proof-166534?style=for-the-badge" alt="Visual proof" /></a>
  <a href="#safety"><img src="https://img.shields.io/badge/Safety-365314?style=for-the-badge" alt="Safety" /></a>
  <a href="#quality"><img src="https://img.shields.io/badge/Quality-334155?style=for-the-badge" alt="Quality" /></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2024-000000?style=flat-square&logo=rust&logoColor=white" alt="Rust 2024" />
  <img src="https://img.shields.io/badge/Axum-0.8-0f766e?style=flat-square" alt="Axum 0.8" />
  <img src="https://img.shields.io/badge/PostgreSQL-17-4169e1?style=flat-square&logo=postgresql&logoColor=white" alt="PostgreSQL 17" />
  <img src="https://img.shields.io/badge/React-19-0ea5e9?style=flat-square&logo=react&logoColor=white" alt="React 19" />
  <img src="https://img.shields.io/badge/CI-.github%2Fworkflows%2Fci.yml-15803d?style=flat-square" alt="Repository CI" />
</p>

> **Forge CI/CD** is a self-hosted Git and pipeline control plane for Base: bare Git hosting, Smart HTTP, push-triggered pipelines, runner execution, artifacts, platform configuration and evidence-oriented operations. It is source-available software, not a hosted public service.

<a name="overview"></a>
## Overview

Forge joins a Git surface, an execution control plane and an operator dashboard without claiming distributed-production guarantees it does not yet provide. The current local Compose implementation is suitable for bounded development and operator-controlled environments; target scope is documented separately.

| Surface | Current behavior | Status |
|---|---|---|
| Git | Bare repositories, Smart HTTP, push-triggered pipelines, branches/tags, compare and pull-request flow. | Current verified |
| Pipelines | Immutable plan snapshots, DAG-compatible jobs, bounded logs, execution attempts, artifacts and cancellation. | Current verified MVP |
| Execution | Embedded Docker/shell runner plus an external `forge-runner` protocol slice with leases and reconciliation. | Current verified MVP |
| Platform | Projects, secrets, environments, protected approvals, schedules, webhooks, notifications, reports and audit. | Current verified MVP |
| Security | Conditional JWT/PAT auth, project membership RBAC, Git checks, CORS configuration and bounded in-process limits. | Current verified MVP |
| Target scope | Tenant isolation, service accounts, external delivery adapters, Kubernetes isolation and distributed guarantees. | Target approved |

`Current verified` means implemented and backed by tests, CI, runtime or runbook evidence. `MVP` means a bounded local control-plane capability, not a distributed guarantee. The exhaustive capability and boundary inventory is [docs/CURRENT_STATE.md](docs/CURRENT_STATE.md).

<a name="capabilities"></a>
## Capabilities

- **Repository control.** Register projects and repositories, browse Git trees and releases, compare branches, open pull requests and trigger work through push hooks.
- **Pipeline evidence.** Persist a plan snapshot, stages/jobs, attempt-owned logs, declared artifacts and terminal status so an operator can inspect what ran.
- **Runner model.** Use the embedded runner for local Compose work or the lease-aware shell `forge-runner` protocol for bounded external execution.
- **Platform operations.** Manage encrypted project secrets, environments/deployments, schedules, outgoing webhooks, local notifications, reports, audit records, users, memberships and API tokens.
- **Interfaces.** Run the React dashboard or use the HTTP-only `cicd-cli`; API and Git contracts are described in [docs/API.md](docs/API.md), [docs/GIT_HOSTING.md](docs/GIT_HOSTING.md) and [docs/CLI.md](docs/CLI.md).

<a name="quick-start"></a>
## Quick Start

Use the repository-local Compose profile for a controlled local environment. The checked-in `.env.example` is a template: set operator-owned credentials and encryption material in an ignored `.env`; never paste real values into commands, committed files or issue text.

```bash
cp .env.example .env
# Edit .env: set the required database password and a unique CICD_SECRETS_KEY.
docker compose up --build -d
curl -fsS http://127.0.0.1:22801/api/v1/health
curl -fsS http://127.0.0.1:22801/api/v1/readiness
```

Repository-local defaults are dashboard `22802`, API `22801` and loopback PostgreSQL `22543`. In the Base umbrella runtime the dashboard/API are published at `7712`/`7711`; those are deployment-specific local coordinates, not a public endpoint. See [docs/ENV.md](docs/ENV.md), [docs/DEVELOPMENT_GUIDE.md](docs/DEVELOPMENT_GUIDE.md) and [docs/OPERATIONS.md](docs/OPERATIONS.md) before operating a shared environment.

<a name="visual-proof"></a>
## Visual Proof

The root README deliberately uses only reviewed synthetic or blank-state assets. The dashboard and repository-browser screenshots are retained in the full registry but are not repeated here because their seed data includes internal-looking topology and remotes.

### Login boundary

![Forge CI/CD login](docs/screenshots/01-login.png)

### Pipeline plan and job evidence

![Forge CI/CD pipeline detail](docs/screenshots/06-pipeline-detail.png)

### Pipeline detail on mobile

![Forge CI/CD pipeline detail on mobile](docs/screenshots/m-pipeline-detail.png)

The mobile image is 375x812 proof of the card layout. Dense pipeline metadata remains necessarily compact at that width. The complete 46-screen route/evidence registry, capture conditions and known mock-only exceptions live in [docs/assets/screens/manifest.md](docs/assets/screens/manifest.md).

<a name="safety"></a>
## Safety Boundaries

- **Auth is conditional.** With an empty `CICD_AUTH_SECRET`, API and dashboard run in trusted-local mode. A shared deployment must set an auth secret, a CORS allowlist and operator-controlled ingress.
- **Secrets stay server-side.** Project secret values are encrypted at rest and only declared names are injected into jobs. Logs use best-effort masking; do not treat it as a substitute for safe job commands and environment policy.
- **Runner isolation is bounded.** The embedded runner and external shell protocol are local execution mechanisms. Kubernetes isolation, advanced runner pools/protected tags and a production runner-zone boundary remain target work.
- **Readiness is specific.** `/api/v1/health` is process liveness; `/api/v1/readiness` also checks PostgreSQL and committed SQLx migration state. Neither asserts every provider, webhook receiver or runner is healthy.
- **Ingress remains operator-owned.** The optional local TLS profile does not create public ingress, ACME, firewall policy or tenant isolation. See [SECURITY.md](SECURITY.md) and [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

<a name="quality"></a>
## Quality and Verification

| Gate | Command |
|---|---|
| Documentation integrity and README contract | `python3 scripts/verify_docs.py --all` |
| Documentation regression tests | `python3 -m unittest scripts.tests.test_verify_docs -v` |
| Backend workspace | `just test-backend` |
| Frontend tests | `just test-frontend` |
| Frontend production build | `just build-frontend` |
| Local health/readiness | `just health` / `just readiness` |
| Browser E2E, accessibility and performance smoke | `cd frontend && pnpm e2e` |
| Secret/SBOM checks | `python3 scripts/scan_secrets.py` / `python3 scripts/generate_sbom.py --check` |

GitHub Actions covers Rust formatting, clippy, workspace/integration tests, release build, frontend contract/test/lint/build, Compose smoke, browser E2E, security scans and documentation checks. The README contract is part of the same docs verification gate; it validates anchors, local assets, safe proof references, placeholder/path leaks and workflow badge targets.

## Documentation Map

- **Current capability boundaries:** [docs/CURRENT_STATE.md](docs/CURRENT_STATE.md), [docs/ROADMAP.md](docs/ROADMAP.md)
- **Operator material:** [docs/OPERATIONS.md](docs/OPERATIONS.md), [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md), [docs/DISASTER_RECOVERY.md](docs/DISASTER_RECOVERY.md)
- **Architecture:** [docs/ARCHITECTURE_INDEX.md](docs/ARCHITECTURE_INDEX.md), [docs/RUNNER_ARCHITECTURE.md](docs/RUNNER_ARCHITECTURE.md), [docs/STORAGE_ARCHITECTURE.md](docs/STORAGE_ARCHITECTURE.md)
- **Contracts:** [docs/API.md](docs/API.md), [docs/DATA_MODEL.md](docs/DATA_MODEL.md), [docs/IMPLEMENTATION_CONTRACTS.md](docs/IMPLEMENTATION_CONTRACTS.md)
- **Security and quality:** [SECURITY.md](SECURITY.md), [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md), [docs/TEST_PLAN.md](docs/TEST_PLAN.md)

<a name="license"></a>
## License

FerrPOINT Proprietary Source-Available Evaluation License v1.0. This is not open source. Viewing and evaluation are allowed under the repository license; commercial, production, resale, redistribution and SaaS/hosting use require a written FerrPOINT license. See [LICENSE](LICENSE), [NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
