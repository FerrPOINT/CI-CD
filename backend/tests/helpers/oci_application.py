"""Disposable actual serving application, not the product executor."""
import http.server
import json
import time
from pathlib import Path

VERSION = Path('/app-version').read_text().strip()

class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        manifest = Path('/forge/manifest.json').read_bytes()
        data = Path('/forge/data.json').read_bytes()
        actual = json.loads(data)
        status, body = 404, b'missing\n'
        if self.path == '/.forge/version':
            status, body = 200, manifest
        elif self.path == '/health':
            if VERSION == 'D' and not getattr(self.server, 'delayed', False):
                self.server.delayed = True
                print('HEALTH_DELAY_ENTERED', flush=True)
                time.sleep(10)
            status, body = (503, b'failed\n') if VERSION == 'B' else (200, b'ok\n')
        elif self.path == '/compatibility':
            status, body = (200, data) if actual.get('schemaVersion') == 1 else (409, b'incompatible\n')
        elif self.path == '/acceptance':
            # Exercise actual payload semantics, distinct from process liveness.
            expected = [{'id': 'task-1', 'state': 'queued'}]
            status, body = (200, b'accepted:task-1:queued\n') if actual.get('records') == expected and VERSION != 'C' else (422, b'rejected\n')
        self.send_response(status)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

http.server.ThreadingHTTPServer(('0.0.0.0', 8000), Handler).serve_forever()
