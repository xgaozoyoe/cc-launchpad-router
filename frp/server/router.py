"""Launch-ID HTTP transport router. Demo forwards supplier Bearer auth; no credit ledger."""
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
from urllib.parse import urlsplit


def resolve(path, method, registry):
    match = re.fullmatch(r'/launches/([a-zA-Z0-9][a-zA-Z0-9_-]{0,79})(/v1/(?:models|chat/completions))', path)
    if not match:
        return None
    launch = registry.get('launches', {}).get(match[1])
    provider = registry.get('providers', {}).get(launch.get('provider_id')) if launch else None
    if not launch or not launch.get('enabled') or not provider or not provider.get('enabled'):
        return None
    if launch.get('status') != 'live':
        return None
    if any(launch.get(k) != provider.get(k) for k in ('model', 'revision', 'precision', 'context')):
        return None
    if (method, match[2]) not in (('GET', '/v1/models'), ('POST', '/v1/chat/completions')):
        return None
    # Host is built from an operator-assigned ID, never supplied by the public caller.
    provider_id = provider.get('tunnel_id', provider['id'])
    if not re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?', provider_id):
        return None
    return {'launch_id': launch['id'], 'host': provider_id + '.providers.internal', 'path': match[2]}


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def error(self, status, message):
        body = json.dumps({'error': message}).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def forward(self):
        try:
            registry = json.loads(Path(os.environ.get('REGISTRY_PATH', '/data/registry.json')).read_text())
        except (OSError, ValueError):
            return self.error(503, 'Launch registry unavailable')
        parsed = urlsplit(self.path)
        route = resolve(parsed.path, self.command, registry) if not parsed.query else None
        if not route:
            return self.error(404, 'Launch route unavailable')
        # Transport demo only: caller supplies supplier key. Real buyers must use the platform gateway/ledger.
        auth = self.headers.get('Authorization', '')
        if not auth.startswith('Bearer ') or len(auth) <= 7:
            return self.error(401, 'Bearer authorization required')
        if self.headers.get('Transfer-Encoding'):
            return self.error(400, 'Chunked request bodies are not supported')
        try:
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 <= length <= 1048576:
                return self.error(413, 'Request body too large')
        except ValueError:
            return self.error(400, 'Invalid Content-Length')
        self.connection.settimeout(180)
        body = self.rfile.read(length) if length else None
        if body is not None and len(body) != length:
            return self.error(400, 'Incomplete body')
        upstream = http.client.HTTPConnection(os.environ.get('FRPS_HOST', 'frps'),
                                              int(os.environ.get('FRPS_HTTP_PORT', '8080')), timeout=180)
        headers_sent = False
        try:
            upstream.request(self.command, route['path'], body=body, headers={
                'Host': route['host'], 'Authorization': auth,
                'Content-Type': self.headers.get('Content-Type', 'application/json'),
                'Accept': self.headers.get('Accept', '*/*')})
            response = upstream.getresponse()
            self.send_response(response.status)
            self.send_header('Content-Type', response.getheader('Content-Type', 'application/json'))
            self.send_header('Cache-Control', 'no-store')
            self.send_header('X-Launch-ID', route['launch_id'])
            self.send_header('Connection', 'close')
            self.end_headers()
            headers_sent = True
            # read1 forwards SSE as it arrives, without buffering a complete response.
            while True:
                chunk = response.read1(65536)
                if not chunk:
                    break
                self.wfile.write(chunk)
                self.wfile.flush()
        except (OSError, http.client.HTTPException):
            if not headers_sent:
                self.error(502, 'Provider inference unavailable')
        finally:
            self.close_connection = True
            upstream.close()

    do_GET = forward
    do_POST = forward
    def log_message(self, *_):
        pass


if __name__ == '__main__':
    ThreadingHTTPServer(('0.0.0.0', 8001), Handler).serve_forever()
