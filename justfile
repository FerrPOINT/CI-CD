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
    docker run --rm --entrypoint /bin/bash -v "{{justfile_directory()}}/backend:/workspace" -w /workspace rust:1.86-bookworm -lc '/usr/local/cargo/bin/cargo test'

test-frontend:
    cd frontend && pnpm test

build-frontend:
    cd frontend && pnpm build

health:
    curl -fsS http://127.0.0.1:22801/api/v1/health

readiness:
    curl -fsS http://127.0.0.1:22801/api/v1/readiness
