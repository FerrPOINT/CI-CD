"""Synthetic Tracker and retained application, only for disposable bridge QA."""
import http.server
import json
import signal
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

mode = sys.argv[1]
data = json.loads(Path('/fixture-input/identities.json').read_text())
if mode == 'application':
    Path('/data/marker').write_text(data['marker']) if not Path('/data/marker').exists() else None

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if mode == 'application':
            result = {'marker': Path('/data/marker').read_text()}
        else:
            url = urlsplit(self.path)
            if url.path == '/api/v1/namespace-projects':
                self.send_error(503, 'Fixture retains its seeded project projection')
                return
            query = parse_qs(url.query)
            assert self.headers.get('Authorization') == 'Bearer synthetic-tracker-reader'
            assert query['registry_instance_id'] == [data['registry']]
            assert query['namespace_id'] == [data['namespace']]
            namespace = {'registry_instance_id': data['registry'], 'namespace_id': data['namespace']}
            if url.path == '/api/v1/namespace-tasks':
                result = [{'namespace': namespace, 'task': {'tracker_instance_id': data['tracker'],
                    'task_id': data['task']}, 'task_key': data.get('task_key', 'FIXTURE-7'), 'label': 'Fixture task'}]
            else:
                assert url.path == '/api/v1/namespace-tasks/' + data['task']
                result = {'schema_version': 1, 'namespace': namespace, 'tracker_instance_id': data['tracker'],
                    'project_id': data['tracker_project'], 'task_id': data['task'],
                    'task_key': data.get('task_key', 'FIXTURE-7'), 'state': 'active'}
        payload = json.dumps(result).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *args):
        pass

# Explicit native normal-exit path for this disposable application's stop.
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
http.server.HTTPServer(('0.0.0.0', 8080), Handler).serve_forever()
