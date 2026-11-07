#!/usr/bin/env python3
"""Prepare dedicated launchpad authorization on the EXISTING FRPS port; do not start/restart services."""
from pathlib import Path
import hashlib,io,json,os,subprocess,tarfile,urllib.request,tomllib
base=Path(__file__).resolve().parents[2]
os.umask(0o077)
private=base/'private';tools=base/'tools/frp-0.71.1';tools.mkdir(parents=True,exist_ok=True)
url='https://github.com/fatedier/frp/releases/download/v0.71.1/frp_0.71.1_linux_amd64.tar.gz'
expected='1d9875210972a5d1a62b2dc4c48df45ab0af05d3ca1b5e494a014428259df803'
archive=tools/'release.tar.gz'
if not archive.exists():archive.write_bytes(urllib.request.urlopen(url,timeout=90).read())
assert hashlib.sha256(archive.read_bytes()).hexdigest()==expected,'Official release digest mismatch'
with tarfile.open(archive,mode='r:gz') as tar:
 for name in ['frpc','frps','LICENSE']:
  data=tar.extractfile('frp_0.71.1_linux_amd64/'+name).read();path=tools/name;path.write_bytes(data);path.chmod(0o700 if name!='LICENSE' else 0o600)
old_path=Path('/home/xgao/frp/frps.toml');old=tomllib.loads(old_path.read_text())
assert old.get('bindPort')==7000 and old.get('vhostHTTPPort')==8080
backup=private/'frps-original.toml'
if not backup.exists():backup.write_bytes(old_path.read_bytes())
token=private/'frps-transport-token'
if not token.exists():
 auth=old['auth']
 if 'token' in auth:token.write_text(auth['token'])
 else:token.write_bytes(Path(auth['tokenSource']['file']['path']).read_bytes())
cert=private/'frps-ca.crt';key=private/'frps-tls.key'
if not cert.exists():
 subprocess.run(['openssl','req','-x509','-newkey','rsa:3072','-nodes','-days','365','-keyout',str(key),'-out',str(cert),'-subj','/CN=riddle.xyz','-addext','subjectAltName=DNS:riddle.xyz,IP:100.109.156.120','-addext','basicConstraints=critical,CA:TRUE'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
helper=base/'deploy/provider_bundle.py'
config={'python':'/usr/bin/python3','helper':str(helper),'server_addr':'riddle.xyz','server_port':7000,'tls_server_name':'riddle.xyz','frpc_binary':str(tools/'frpc'),'frpc_sha256':hashlib.sha256((tools/'frpc').read_bytes()).hexdigest(),'ca_file':str(cert),'transport_token_file':str(token),'license_file':str(tools/'LICENSE'),'credential_dir':str(private/'provider-credentials')}
(private/'provider-bundles.json').write_text(json.dumps(config,indent=2))
q=json.dumps
(private/'frps.toml').write_text(f'''bindAddr = "0.0.0.0"
bindPort = 7000
proxyBindAddr = "127.0.0.1"
vhostHTTPPort = 8080
vhostHTTPTimeout = 180
auth.method = "token"
auth.tokenSource.type = "file"
auth.tokenSource.file.path = {q(str(token))}
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.force = true
transport.tls.certFile = {q(str(cert))}
transport.tls.keyFile = {q(str(key))}
log.to = {q(str(private/'frps.log'))}
log.level = "info"
detailedErrorsToClient = false
[[httpPlugins]]
name = "launchpad-provider-authorization"
addr = "127.0.0.1:9002"
path = "/internal/frp/handler"
ops = ["Login", "NewProxy"]
''')
for p in [token,cert,key,private/'provider-bundles.json',private/'frps.toml',backup]:p.chmod(0o600)
subprocess.run([str(tools/'frps'),'verify','-c',str(private/'frps.toml')],check=True)
print('Prepared project-scoped TLS, authorization and verified FRPC 0.71.1 assets; original FRPS config preserved. Services were not restarted.')
