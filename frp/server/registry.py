"""Small private FRP authorization plugin and registration list; not a public API."""
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import time
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def read_registry():
    return json.loads(Path(os.environ.get('REGISTRY_PATH', '.secrets/frp-server/registry.json')).read_text())


def identify(content, entries):
    provider_id = content.get('user')
    token = content.get('metas', {}).get('provider_token', '')
    item = entries.get(provider_id)
    if not item or not item.get('enabled') or not isinstance(token, str):
        return None
    if item.get('status') == 'awaiting-rpc' and time.time() >= item.get('verification_deadline', 0):
        return None
    digest = hashlib.sha256(token.encode()).hexdigest()
    return item if hmac.compare_digest(digest, item['credential_sha256']) else None


def authorize(op, content, entries):
    if op == 'Login':
        allowed = identify(content, entries) is not None
    elif op == 'NewProxy':
        user = content.get('user', {})
        item = identify(user, entries) if isinstance(user, dict) else None
        provider_id = user.get('user') if isinstance(user, dict) else None
        route_id = item.get('tunnel_id', provider_id) if item else None
        allowed = bool(item and content.get('proxy_type') == 'http'
                       and content.get('proxy_name') == provider_id + '.inference'
                       and content.get('subdomain') == route_id
                       and not content.get('custom_domains')
                       and not content.get('locations')
                       and not content.get('http_user') and not content.get('http_pwd')
                       and not content.get('host_header_rewrite')
                       and not content.get('group') and not content.get('group_key'))
    else:
        allowed = False
    return {'reject': not allowed, 'reject_reason': '' if allowed else 'Provider or route not authorized', 'unchange': True}


def provider_list(entries):
    return [{k: item[k] for k in ('id', 'model', 'revision', 'precision', 'context', 'enabled')}
            | {'status': 'registered-unverified'} for item in entries.values()]


class Handler(BaseHTTPRequestHandler):
    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path not in ('/providers', '/launches'):
            return self.reply(404, {'error': 'Not found'})
        try:
            data = read_registry()
            self.reply(200, {'providers': provider_list(data['providers'])} if self.path == '/providers' else {'launches': list(data['launches'].values())})
        except (OSError, ValueError, KeyError, TypeError):
            self.reply(503, {'error': 'Registry unavailable'})

    def do_POST(self):
        try:
            parsed = urlsplit(self.path)
            if parsed.path != '/handler':
                return self.reply(404, {'error': 'Not found'})
            length = int(self.headers.get('Content-Length', '0'))
            if not 0 < length <= 65536:
                return self.reply(400, {'error': 'Invalid request size'})
            request = json.loads(self.rfile.read(length))
            content = request['content']
            if not isinstance(content, dict):
                raise ValueError('Invalid content')
            op = parse_qs(parsed.query).get('op', [''])[0]
            self.reply(200, authorize(op, content, read_registry()['providers']))
        except (OSError, ValueError, KeyError, TypeError, AttributeError):
            self.reply(200, {'reject': True, 'reject_reason': 'Invalid authorization request', 'unchange': True})

    def log_message(self, *_):
        pass  # Never log provider metadata or credentials.


if __name__ == '__main__':
    ThreadingHTTPServer(('0.0.0.0', 9000), Handler).serve_forever()
