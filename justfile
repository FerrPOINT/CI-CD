set shell := ["bash", "-uc"]

default:
    @just --list

up:
    docker compose up --build -d

down:
    docker compose down

logs:
    docker compose logs -f

# Run inside the prepared pinned development environment.
test-backend:
    cd backend && cargo test --locked -p cicd-domain --lib

# Manual full workspace acceptance; database/CLI cases remain in release.yml.
test-backend-all:
    cd backend && cargo test --locked --workspace

test-frontend:
    cd frontend && pnpm test:unit

test-frontend-all:
    cd frontend && pnpm test:all

build-frontend:
    cd frontend && pnpm build

health:
    curl -fsS http://127.0.0.1:22801/api/v1/health

readiness:
    curl -fsS http://127.0.0.1:22801/api/v1/readiness
