#!/usr/bin/env python3
"""M6: local alert sink — receives Alertmanager webhooks, appends JSON to a
log file and stdout. Replace with a real channel (Telegram bot, SMTP) by
setting FORGE_ALERT_WEBHOOK_URL in docker-compose.local.yml."""
import json, datetime
from http.server import BaseHTTPRequestHandler, HTTPServer

LOG = "/var/log/alerts.jsonl"

class H(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(n)
        entry = {"received": datetime.datetime.utcnow().isoformat() + "Z",
                 "payload": json.loads(body or b"{}")}
        line = json.dumps(entry, ensure_ascii=False)
        with open(LOG, "a") as f:
            f.write(line + "\n")
        print(line, flush=True)
        self.send_response(200); self.end_headers(); self.wfile.write(b"ok")
    def log_message(self, *a): pass

HTTPServer(("0.0.0.0", 7801), H).serve_forever()
