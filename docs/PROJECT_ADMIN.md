# Project Administration — Forge CI/CD

## 1. Current MVP

Project is the control-plane card for a Git repository. It owns pipelines and the current project-scoped platform resources: secrets, environments/deployments, schedules, webhooks/notifications and reports.

### Implemented API

| Method | Path | Behavior |
|---|---|---|
| GET | `/api/v1/projects` | List projects, newest first |
| POST | `/api/v1/projects` | Create `{name, repository_url, default_branch?}` |
| GET | `/api/v1/projects/{id}` | Get one project |
| PATCH | `/api/v1/projects/{id}` | Partial change name/repository URL/default branch; empty body rejected |
| DELETE | `/api/v1/projects/{id}` | Deletes project and cascade-owned pipeline data |
| GET/POST | `/api/v1/projects/{id}/pipelines` | List/trigger pipelines |

`name` and `repository_url` are required on create; duplicate name currently surfaces as a database error. Proper conflict envelope, membership checks and Git URL policy are target work: `docs/DELIVERY_ARCHITECTURE.md`, `docs/AUTHORIZATION.md`.

## 2. Project resource map

| Resource | Path | MVP status |
|---|---|---|
| Pipelines | `/projects/{id}/pipelines` | Working |
| Secrets | `/projects/{id}/secrets` | AES-GCM storage; no runner injection/masking |
| Environments | `/projects/{id}/environments` | Metadata + deployment records |
| Schedules | `/projects/{id}/schedules` | Configuration only; no scheduler |
| Webhooks | `/projects/{id}/webhooks` | Configuration only; no delivery worker |
| Notifications | `/projects/{id}/notifications` | Configuration only; no sender |
| Reports | `/projects/{id}/reports/summary` | Aggregates from pipelines |

## 3. UI navigation

`/projects` provides create/edit/delete and links to repositories/pipelines and project resources. Related routes are in `docs/ROUTING.md`; screenshots: `03-projects`, `05-pipelines`, `14-secrets`, `16-environments`, `17-schedules`, `18-webhooks`, `19-reports`.

## 4. Target policy

Current project data is instance-wide because auth is absent. Target architecture adds tenant/project membership and role checks before every project-scoped query, artifact download, Git transport and secret access. See `docs/AUTHORIZATION.md`.

## References

- `docs/API.md`
- `docs/DATA_MODEL.md`
- `docs/ROUTING.md`
- `docs/AUTHORIZATION.md`
- `docs/RUNNER_ARCHITECTURE.md`
