#!/usr/bin/env python3
"""Regression coverage for the Forge TLS reverse-proxy profile."""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PROFILE = ROOT / "docker-compose.tls.yml"
CADDYFILE = ROOT / "deploy/Caddyfile"


class TlsProfileTests(unittest.TestCase):
    def test_ca_export_helper_keeps_the_compose_profile_explicit(self) -> None:
        helper = (ROOT / "scripts/export-tls-ca.sh").read_text(encoding="utf-8")
        self.assertIn('--project-name NAME', helper)
        self.assertIn('compose_files=(-p "$project_name" "${compose_files[@]}")', helper)
        self.assertIn('docker compose "${compose_files[@]}" cp', helper)
        self.assertIn("/data/caddy/pki/authorities/local/root.crt", helper)
        self.assertIn("--output is required", helper)

    def test_profile_keeps_direct_forge_http_ports_private(self) -> None:
        profile = PROFILE.read_text(encoding="utf-8")
        self.assertEqual(profile.count("ports: !reset []"), 2)
        self.assertIn("CICD_AUTH_SECRET: ${CICD_AUTH_SECRET:?", profile)
        self.assertIn("CICD_AUTH_COOKIE_SECURE: \"true\"", profile)
        self.assertIn("CICD_CORS_ALLOWED_ORIGINS: https://${CICD_TLS_HOST", profile)
        self.assertIn('"127.0.0.1:${CICD_TLS_HTTPS_PORT:-22443}', profile)
        self.assertIn("healthcheck:", profile)
        self.assertIn('test: ["CMD", "caddy", "version"]', profile)
        self.assertIn("caddy:2.10.2-alpine@sha256:", profile)

    def test_caddyfile_terminates_internal_tls_and_never_exposes_metrics(self) -> None:
        caddyfile = CADDYFILE.read_text(encoding="utf-8")
        self.assertIn("tls internal", caddyfile)
        self.assertIn("trusted_proxies static private_ranges", caddyfile)
        self.assertIn("reverse_proxy backend:22801", caddyfile)
        self.assertIn("reverse_proxy frontend:80", caddyfile)
        self.assertIn("X-Forwarded-Proto https", caddyfile)
        self.assertNotIn("/metrics", caddyfile)


if __name__ == "__main__":
    unittest.main()
