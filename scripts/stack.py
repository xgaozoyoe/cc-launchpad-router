#!/usr/bin/env python3
"""Single-command local stack and reproducible single-host configuration."""
import argparse
import base64
import os
from pathlib import Path
import secrets
import shutil
import socket
import signal
import subprocess
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
PRIVATE = ROOT / '.secrets' / 'stack'

def write_new(path, value, mode=0o600):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(fd, 'w') as f:
        f.write(value)

def setup(origin, local=False):
    PRIVATE.mkdir(parents=True, exist_ok=True, mode=0o700)
    if (PRIVATE / 'stack.env').exists():
        return
    for name, value in [('backend-hmac.key', secrets.token_hex(32)), ('mongo-key', base64.b64encode(secrets.token_bytes(512)).decode())]:
        if not (PRIVATE / name).exists():
            write_new(PRIVATE / name, value + '\n')
    root_password, app_password = secrets.token_hex(32), secrets.token_hex(32)
    values = {
        'APP_ORIGIN': origin,
        'AUTH_SECRET': secrets.token_hex(32),
        'AUTH_ACCESS_MODE': 'public',
        'GOOGLE_CLIENT_ID': '', 'GOOGLE_CLIENT_SECRET': '',
        'MONGO_INITDB_ROOT_USERNAME': 'root',
        'MONGO_INITDB_ROOT_PASSWORD': root_password,
        'MONGO_APP_PASSWORD': app_password,
        'MONGODB_URI': f'mongodb://launchpad:{app_password}@mongo:27017/launchpad?replicaSet=rs0&authSource=launchpad',
        'MONGODB_DATABASE': 'launchpad',
    }
    if local:
        values.update(LOCAL_TEST_ACCOUNT='1', NODE_ENV='development')
    write_new(PRIVATE / 'stack.env', ''.join(f'{k}={v}\n' for k, v in values.items()))
    print('Created private configuration; no credentials printed.')

def run(command, **kwargs):
    subprocess.run(command, cwd=ROOT, check=True, **kwargs)

def ready(url, process):
    for _ in range(120):
        if process.poll() is not None:
            raise RuntimeError('A stack process exited; inspect .secrets/stack/*.log')
        try:
            with urllib.request.urlopen(url, timeout=1) as r:
                if r.status == 200:
                    return
        except Exception:
            time.sleep(.5)
    raise RuntimeError('Service startup timed out; inspect private logs.')

def local():
    setup('http://localhost:4174', True)
    values = dict(line.split('=', 1) for line in (PRIVATE / 'stack.env').read_text().splitlines() if line and not line.startswith('#'))
    if values.get('LOCAL_TEST_ACCOUNT') != '1' or values.get('APP_ORIGIN') != 'http://localhost:4174':
        raise RuntimeError('Configuration is not for the local test stack. Use a separate checkout for production.')
    for port in (27188, 9001, 4174):
        with socket.socket() as check:
            try:
                check.bind(('127.0.0.1', port))
            except OSError as exc:
                raise RuntimeError(f'Port {port} is in use; stop the existing local stack first.') from exc
    mongod = shutil.which('mongod') or next((str(p) for p in Path('/opt/homebrew/opt').glob('mongodb-community*/bin/mongod')), None)
    if not mongod or not shutil.which('cargo') or not shutil.which('node'):
        raise RuntimeError('Install MongoDB, Rust stable and Node.js >=22 before local startup.')
    if not (ROOT / 'apps/launchpad/node_modules').exists():
        run(['npm', '--prefix', 'apps/launchpad', 'ci'])
    run(['npm', '--prefix', 'apps/launchpad', 'run', 'build'])
    env = {**os.environ, **values, 'CARGO_TARGET_DIR': str(PRIVATE / 'target')}
    run(['cargo', '+stable', 'build', '--manifest-path', 'central/Cargo.toml', '--locked'], env=env)
    data = PRIVATE / 'mongo-data'
    data.mkdir(exist_ok=True)
    children, logs = [], []
    def start(command, label, child_env):
        log = open(PRIVATE / f'{label}.log', 'a')
        logs.append(log)
        process = subprocess.Popen(command, cwd=ROOT, env=child_env, stdout=log, stderr=log)
        children.append(process)
        return process
    try:
        start([mongod, '--dbpath', str(data), '--replSet', 'rs0', '--port', '27188', '--bind_ip', '127.0.0.1'], 'mongo', env)
        env.update(MONGODB_URI='mongodb://127.0.0.1:27188/?directConnection=true&serverSelectionTimeoutMS=10000', CENTRAL_HMAC_KEY_FILE=str(PRIVATE / 'backend-hmac.key'), BIND_ADDRESS='127.0.0.1:9001')
        time.sleep(1)
        run([str(PRIVATE / 'target/debug/init-local-mongo')], env=env)
        central = start([str(PRIVATE / 'target/debug/cc-central')], 'central', env)
        ready('http://127.0.0.1:9001/healthz', central)
        env.update(CENTRAL_API_ORIGIN='http://127.0.0.1:9001', PORT='4174', BIND_HOST='127.0.0.1', STATIC_DIR=str(ROOT / 'apps/launchpad/dist'))
        web = start(['node', str(ROOT / 'apps/launchpad/.server-check/server/standalone.js')], 'web', env)
        ready('http://localhost:4174/api/runtime', web)
        print('Local integrated workspace: http://localhost:4174', flush=True)
        print('Local test account; real MongoDB drafts; stake/sales remain disabled. Ctrl+C stops services.', flush=True)
        while all(p.poll() is None for p in children):
            time.sleep(1)
        raise RuntimeError('A stack process exited; inspect private logs.')
    finally:
        for p in reversed(children):
            if p.poll() is None:
                p.terminate()
        for p in reversed(children):
            try:
                p.wait(timeout=15)
            except subprocess.TimeoutExpired:
                p.kill()
        for log in logs:
            log.close()

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['local', 'setup', 'up', 'down'])
    parser.add_argument('--origin', default='')
    args = parser.parse_args()
    def stop(_signal, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, stop)
    try:
        if args.action == 'local':
            local()
        elif args.action in ('up', 'down'):
            if not (PRIVATE / 'stack.env').exists():
                parser.error('Run setup --origin https://YOUR_DOMAIN first.')
            command = ['docker', 'compose', '--env-file', str(PRIVATE / 'stack.env'), '-f', 'deploy/stack/compose.yaml']
            run(command + (['up', '--build', '-d'] if args.action == 'up' else ['down']))
        else:
            from urllib.parse import urlparse
            u = urlparse(args.origin)
            if u.scheme != 'https' or not u.hostname or u.path not in ('', '/') or u.query or u.fragment or u.username:
                parser.error('--origin must be an HTTPS origin')
            setup(args.origin.rstrip('/'))
            print('Add Google OAuth credentials to .secrets/stack/stack.env, then run Docker Compose.')
    except KeyboardInterrupt:
        pass
