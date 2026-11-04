"""Offline provisioning for the platform FRPS operator. Secrets never go to stdout."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import subprocess

MODEL = {'model': 'Qwen/Qwen3-8B-AWQ', 'revision': '93eb32ea9ef98cc266b85563ad0dd29e5f2940b9',
         'precision': 'AWQ 4-bit', 'context': 4096}
ROOT = Path(__file__).resolve().parents[1]


def write_private(path, text):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        output.write(text)


def dns(value):
    if len(value) > 253 or not all(re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?', x) for x in value.split('.')):
        raise argparse.ArgumentTypeError('Use a lowercase DNS hostname without scheme or port')
    return value


def initialize(args):
    dest = ROOT / '.secrets/frp-server'
    if dest.exists():
        raise ValueError('Server configuration already exists; it was not overwritten')
    dest.mkdir(parents=True, mode=0o700)
    token = secrets.token_urlsafe(32)
    config = {'server': args.server, 'domain': args.domain, 'providers': {}}
    write_private(dest / 'registry.json', json.dumps(config, indent=2) + '\n')
    # Self-signed CA/server certificate; providers pin this certificate and its DNS name.
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:3072', '-nodes', '-days', '365',
                    '-keyout', str(dest / 'server.key'), '-out', str(dest / 'ca.crt'),
                    '-subj', '/CN=' + args.server, '-addext', 'subjectAltName=DNS:' + args.server,
                    '-addext', 'basicConstraints=critical,CA:TRUE'], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    (dest / 'server.key').chmod(0o600)
    write_private(dest / 'token', token)
    write_private(dest / 'frps.toml', f'''bindPort = 7000
vhostHTTPPort = 8080
vhostHTTPTimeout = 180
subDomainHost = {json.dumps(args.domain)}
auth.method = "token"
auth.tokenSource.type = "file"
auth.tokenSource.file.path = "/etc/frp/token"
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.force = true
transport.tls.certFile = "/etc/frp/ca.crt"
transport.tls.keyFile = "/etc/frp/server.key"
webServer.addr = "0.0.0.0"
webServer.port = 7500
webServer.user = "operator"
webServer.password = {json.dumps(secrets.token_urlsafe(32))}
log.to = "console"
log.level = "warn"
detailedErrorsToClient = false
[[httpPlugins]]
name = "provider-authorization"
addr = "registry:9000"
path = "/handler"
ops = ["Login", "NewProxy"]
''')
    write_private(dest / 'Caddyfile', '# Add providers using scripts/frp_admin.py add.\n')
    print('Initialized private server config. Keep .secrets/frp-server private and backed up.')


def caddy_config(registry):
    # Explicit host blocks let Caddy issue individual certs without a wildcard DNS plugin.
    return '\n'.join(f'''{item['id']}.{registry['domain']} {{
    @api path /v1/models /v1/chat/completions
    handle @api {{
        reverse_proxy frps:8080 {{
            flush_interval -1
        }}
    }}
    handle {{
        respond "Not found" 404
    }}
}}
''' for item in registry['providers'].values() if item['enabled'])


def add_provider(args):
    if not re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,38}[a-z0-9])?', args.id):
        raise ValueError('Provider ID must be 1–40 lowercase letters, numbers or hyphens')
    server = ROOT / '.secrets/frp-server'
    path = server / 'registry.json'
    registry = json.loads(path.read_text())
    bundle = ROOT / '.secrets/frp-bundles' / args.id
    if args.id in registry['providers'] or bundle.exists():
        raise ValueError('Provider already exists; no credentials were overwritten')
    credential = secrets.token_urlsafe(32)
    registry['providers'][args.id] = {'id': args.id, **MODEL, 'enabled': True,
        'credential_sha256': hashlib.sha256(credential.encode()).hexdigest(),
        'endpoint': f"https://{args.id}.{registry['domain']}/v1"}
    write_private(bundle / 'ca.crt', (server / 'ca.crt').read_text())
    write_private(bundle / 'token', (server / 'token').read_text())
    write_private(bundle / 'frpc.toml', f'''serverAddr = {json.dumps(registry['server'])}
serverPort = 7000
user = {json.dumps(args.id)}
loginFailExit = false
metadatas.provider_token = {json.dumps(credential)}
auth.method = "token"
auth.tokenSource.type = "file"
auth.tokenSource.file.path = "/etc/frp/token"
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.enable = true
transport.tls.trustedCaFile = "/etc/frp/ca.crt"
transport.tls.serverName = {json.dumps(registry['server'])}
log.to = "console"
log.level = "warn"
[[proxies]]
name = "inference"
type = "http"
localIP = "inference"
localPort = 8000
subdomain = {json.dumps(args.id)}
''')
    temp = path.with_suffix('.tmp')
    write_private(temp, json.dumps(registry, indent=2) + '\n')
    temp.replace(path)
    (server / 'Caddyfile').write_text(caddy_config(registry))
    print('Issued private provider bundle:', bundle.relative_to(ROOT))
    print('Assigned API:', registry['providers'][args.id]['endpoint'])
    print('Reload central Caddy after adding a provider. No inference API key is included.')


def main():
    parser = argparse.ArgumentParser()
    commands = parser.add_subparsers(dest='command', required=True)
    init = commands.add_parser('init')
    init.add_argument('--server', type=dns, required=True)
    init.add_argument('--domain', type=dns, required=True)
    add = commands.add_parser('add')
    add.add_argument('--id', required=True)
    commands.add_parser('list')
    args = parser.parse_args()
    if args.command == 'init':
        initialize(args)
    elif args.command == 'add':
        add_provider(args)
    else:
        registry = json.loads((ROOT / '.secrets/frp-server/registry.json').read_text())
        for item in registry['providers'].values():
            print(item['id'], item['model'], item['endpoint'], 'registered-unverified')


if __name__ == '__main__':
    os.umask(0o077)
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit('Provisioning failed: ' + str(error))
