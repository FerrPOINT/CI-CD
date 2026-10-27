set shell := ["bash", "-uc"]

default:
    @just --list

up:
    docker compose up --build -d

down:
    docker compose down

logs:
    docker compose logs -f

test-backend:
    cd backend && cargo test --locked -p cicd-domain --lib

# Manual full gate in the prepared pinned Linux environment.
test-backend-all:
    cd backend && cargo test --locked --workspace

test-frontend:
    cd frontend && pnpm test

build-frontend:
    cd frontend && pnpm build

build-backend:
    python3 scripts/build.py backend

health:
    curl -fsS http://127.0.0.1:22801/api/v1/health

readiness:
    curl -fsS http://127.0.0.1:22801/api/v1/readiness
