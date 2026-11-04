"""Optional real FRP integration: FRP_BIN_DIR must contain official frpc/frps binaries."""
import importlib.util
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


@unittest.skipUnless(os.environ.get('FRP_BIN_DIR'), 'Set FRP_BIN_DIR to test a real FRP tunnel')
class TunnelTest(unittest.TestCase):
    def test_tunnel_preserves_api_auth_and_denies_route_takeover(self):
        admin = load('integration_admin', 'scripts/frp_admin.py')
        registry = load('integration_registry', 'frp/server/registry.py')
        binaries = Path(os.environ['FRP_BIN_DIR'])
        with tempfile.TemporaryDirectory() as tmp, patch.object(admin, 'ROOT', Path(tmp)):
            tmp = Path(tmp)
            class Args:
                server = 'frps.example.com'
                api_domain = 'api.example.com'
                id = 'gpu-001'
            admin.initialize(Args())
            admin.add_provider(Args())
            Args.id = 'launch-001'
            Args.provider = 'gpu-001'
            admin.add_launch(Args())
            Args.id = 'gpu-001'
            private = tmp / '.secrets/frp-server'
            bundle = tmp / '.secrets/frp-bundles/gpu-001'
            registry.read_registry = lambda: json.loads((private / 'registry.json').read_text())
            policy = ThreadingHTTPServer(('127.0.0.1', 0), registry.Handler)
            class Inference(BaseHTTPRequestHandler):
                def do_GET(self):
                    body = json.dumps({'data': [{'id': 'qwen3-8b-awq'}]}).encode()
                    if self.headers.get('Authorization') != 'Bearer inference-test-key':
                        self.send_response(401)
                        self.end_headers()
                        return
                    self.send_response(200)
                    self.send_header('Content-Length', str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                def do_POST(self):
                    self.rfile.read(int(self.headers.get('Content-Length', 0)))
                    if self.headers.get('Authorization') != 'Bearer inference-test-key':
                        self.send_response(401)
                        self.end_headers()
                        return
                    body = b'data: {"choices":[{"delta":{"content":"hello"}}]}\n\ndata: [DONE]\n\n'
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/event-stream')
                    self.send_header('Content-Length', str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                def log_message(self, *_):
                    pass
            inference = ThreadingHTTPServer(('127.0.0.1', 0), Inference)
            for server in (policy, inference):
                threading.Thread(target=server.serve_forever, daemon=True).start()
            control, http, dashboard = free_port(), free_port(), free_port()
            server_text = (private / 'frps.toml').read_text().replace('bindPort = 7000', f'bindAddr = "127.0.0.1"\nbindPort = {control}')
            server_text = server_text.replace('vhostHTTPPort = 8080', f'vhostHTTPPort = {http}')
            server_text = server_text.replace('webServer.port = 7500', f'webServer.port = {dashboard}')
            server_text = server_text.replace('/etc/frp/', str(private) + '/').replace('registry:9000', f'127.0.0.1:{policy.server_port}')
            client_text = (bundle / 'frpc.toml').read_text().replace('serverAddr = "frps.example.com"', 'serverAddr = "127.0.0.1"')
            client_text = client_text.replace('serverPort = 7000', f'serverPort = {control}').replace('/etc/frp/', str(bundle) + '/')
            client_text = client_text.replace('localIP = "inference"', 'localIP = "127.0.0.1"').replace('localPort = 8000', f'localPort = {inference.server_port}')
            sp, cp = tmp / 'frps.toml', tmp / 'frpc.toml'
            sp.write_text(server_text)
            cp.write_text(client_text)
            for binary, config in (('frps', sp), ('frpc', cp)):
                result = subprocess.run([str(binaries / binary), 'verify', '-c', str(config)], capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
            processes = []
            log = open(tmp / 'frp.log', 'w+')
            try:
                processes.append(subprocess.Popen([str(binaries/'frps'), '-c', str(sp)], stdout=log, stderr=log))
                processes.append(subprocess.Popen([str(binaries/'frpc'), '-c', str(cp)], stdout=log, stderr=log))
                def call(host, auth=True):
                    headers = {'Host': host}
                    if auth:
                        headers['Authorization'] = 'Bearer inference-test-key'
                    with urlopen(Request(f'http://127.0.0.1:{http}/v1/models', headers=headers), timeout=2) as response:
                        return json.load(response)
                result = None
                for _ in range(60):
                    try:
                        result = call('gpu-001.providers.internal')
                        break
                    except (URLError, TimeoutError, ConnectionError):
                        time.sleep(.1)
                self.assertEqual(result, {'data': [{'id': 'qwen3-8b-awq'}]}, 'Real FRP tunnel did not become ready')
                with self.assertRaises(HTTPError) as error:
                    call('gpu-001.providers.internal', auth=False)
                self.assertEqual(error.exception.code, 401)
                # Exercise the uniform launch-ID API through the real FRP tunnel.
                router = load('integration_router', 'frp/server/router.py')
                public = ThreadingHTTPServer(('127.0.0.1', 0), router.Handler)
                threading.Thread(target=public.serve_forever, daemon=True).start()
                try:
                    with patch.dict(os.environ, {'REGISTRY_PATH': str(private / 'registry.json'),
                            'FRPS_HOST': '127.0.0.1', 'FRPS_HTTP_PORT': str(http)}):
                        base = f'http://127.0.0.1:{public.server_port}/launches/launch-001/v1'
                        with urlopen(Request(base + '/models', headers={'Authorization': 'Bearer inference-test-key'})) as reply:
                            self.assertEqual(reply.headers['X-Launch-ID'], 'launch-001')
                            self.assertEqual(json.load(reply), result)
                        with urlopen(Request(base + '/chat/completions', data=b'{"stream":true}',
                                headers={'Authorization': 'Bearer inference-test-key', 'Content-Type': 'application/json'})) as reply:
                            self.assertIn(b'data: [DONE]', reply.read())
                            self.assertEqual(reply.headers['Content-Type'], 'text/event-stream')
                        with self.assertRaises(HTTPError) as error:
                            urlopen(Request(base.replace('launch-001', 'wrong-launch') + '/models',
                                    headers={'Authorization': 'Bearer inference-test-key'}))
                        self.assertEqual(error.exception.code, 404)
                finally:
                    public.shutdown()
                    public.server_close()
                # Stop the legitimate client, then try to claim another provider's route with its valid identity.
                processes[-1].terminate()
                processes[-1].wait(timeout=5)
                bad = tmp / 'bad.toml'
                bad.write_text(client_text.replace('subdomain = "gpu-001"', 'subdomain = "gpu-002"'))
                processes.append(subprocess.Popen([str(binaries/'frpc'), '-c', str(bad)], stdout=log, stderr=log))
                denied = False
                for _ in range(60):
                    log.flush()
                    if 'Provider or route not authorized' in (tmp / 'frp.log').read_text():
                        denied = True
                        break
                    time.sleep(.1)
                self.assertTrue(denied, 'Route takeover did not produce an authorization denial')
                with self.assertRaises(HTTPError) as error:
                    call('gpu-002.providers.internal')
                self.assertEqual(error.exception.code, 404)
            finally:
                for process in processes:
                    if process.poll() is None:
                        process.terminate()
                    process.wait(timeout=5)
                for server in (policy, inference):
                    server.shutdown()
                    server.server_close()
                log.close()


if __name__ == '__main__':
    unittest.main()
