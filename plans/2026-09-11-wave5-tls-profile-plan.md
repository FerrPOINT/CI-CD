# Wave 5 O: TLS profile for Forge CI/CD

**Status 2026-09-11: accepted scope, implementation in progress.**

## Goal

Deliver the selected production-facing transport increment without changing the trusted-local development path: a Caddy reverse-proxy profile terminates HTTPS with its own internal CA, keeps PostgreSQL and direct Forge backend/frontend ports private, sets the existing secure-cookie and exact CORS configuration, and provides repeatable trust and verification procedures.

## Scope

- Add a separate Compose override for the existing Forge stack, not a second application runtime.
- Add a pinned Caddy image and a Caddyfile that routes Dashboard, REST API, and Git Smart HTTP over one HTTPS origin.
- Reset direct backend/frontend host port publication in the TLS profile; PostgreSQL remains private to localhost only.
- Force trusted proxy headers from Caddy, set baseline browser security headers, and do not publish `/metrics` through the public proxy.
- Provide an explicit internal-CA export command and a no-secret sample configuration.
- Add static regression tests and run a live disposable Compose verification with an exported CA.

## Non-goals

- No public DNS, ACME, firewall, system trust-store change, secret-manager migration, tenant isolation, runner mTLS, or external ingress exposure.
- No change to the isolated development Compose defaults.
- No claim that the TLS profile alone makes the remaining production target work complete.

## TDD checkpoints

1. Add tests that require a pinned Caddy profile, no direct backend/frontend ports, protected upstream headers, and an internal TLS issuer.
2. Implement `docker-compose.tls.yml`, `deploy/Caddyfile`, and CA export helper.
3. Document configuration, startup, rollback, trust boundary, and verification.
4. Run static checks, Compose render validation, backend/frontend checks, and a disposable live HTTPS smoke using only generated local values.

## Acceptance criteria

- `docker compose -f docker-compose.yml -f docker-compose.tls.yml config -q` succeeds with generated test-only values.
- `https://forge.localhost:<configured-port>/` returns Dashboard content after trusting/exporting the Caddy internal CA.
- `/api/v1/health`, `/api/v1/readiness`, and Git proxy routes work through HTTPS; direct backend/frontend ports are not published by the TLS profile.
- Login cookies include `Secure` when the TLS profile is enabled.
- Documentation and regression tests prevent accidental re-exposure of backend/frontend and regression to empty CORS/unsafe cookies in the profile.
