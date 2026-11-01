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
    python3 -B ../services-base/scripts/execution_slot.py -- cargo test --manifest-path backend/Cargo.toml --locked -p cicd-domain --lib

test-frontend:
    cd frontend && pnpm test

build-frontend:
    python3 -B ../services-base/scripts/execution_slot.py -- pnpm --dir frontend build

# No arguments selects changed container services; --all is intentional full scope.
build *services:
    python3 -B scripts/build.py {{services}}

health:
    curl -fsS http://127.0.0.1:22801/api/v1/health

readiness:
    curl -fsS http://127.0.0.1:22801/api/v1/readiness

repository-hygiene:
    python3 -B scripts/check_repository_hygiene.py --index

repository-hygiene-ref:
    python3 -B scripts/check_repository_hygiene.py --ref HEAD
