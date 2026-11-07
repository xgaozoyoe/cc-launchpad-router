"""Private, backend-only deterministic bundle builder. Never run with public stdout logging."""
import base64,fcntl,hashlib,io,json,os,re,secrets,tarfile
from pathlib import Path

def build(config,project):
    pid=project['project_id'];provider=project['provider_id'];launch=project['launch_id']
    if not re.fullmatch(r'[a-f0-9-]{36}',pid) or not re.fullmatch(r'0x[a-f0-9]{64}',provider) or not re.fullmatch(r'launch-[a-f0-9-]{36}',launch):raise ValueError('Invalid identifiers')
    binary=Path(config['frpc_binary']).read_bytes()
    if hashlib.sha256(binary).hexdigest()!=config['frpc_sha256']:raise ValueError('Binary checksum mismatch')
    cert=Path(config['ca_file']).read_bytes();transport=Path(config['transport_token_file']).read_bytes().strip()
    private=Path(config['credential_dir']);private.mkdir(mode=0o700,parents=True,exist_ok=True)
    fd=os.open(private/(pid+'.lock'),os.O_RDWR|os.O_CREAT,0o600)
    with os.fdopen(fd,'w') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        key=private/(pid+'.key')
        if not key.exists():
            f=os.open(key,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
            with os.fdopen(f,'w') as out:out.write(secrets.token_urlsafe(32))
        token=key.read_text().strip()
    alias='p-'+hashlib.sha256(provider.encode()).hexdigest()[:40]+'.launchpad.internal'
    q=json.dumps
    toml=f'''serverAddr = {q(config['server_addr'])}
serverPort = {int(config['server_port'])}
user = {q(provider)}
metadatas.provider_token = {q(token)}
auth.method = "token"
auth.tokenSource.type = "file"
auth.tokenSource.file.path = "./private/transport-token"
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.enable = true
transport.tls.trustedCaFile = "./certs/ca.crt"
transport.tls.serverName = {q(config['tls_server_name'])}
log.to = "./logs/frpc.log"
log.level = "info"
[[proxies]]
name = "inference"
type = "http"
localIP = "127.0.0.1"
localPort = 8000
customDomains = [{q(alias)}]
'''
    manager='''import fcntl,json,os,subprocess,sys,time
from pathlib import Path
r=Path(__file__).resolve().parent
os.chdir(r)
os.umask(0o077)
(r/'logs').mkdir(exist_ok=True)
with open(r/'private/manager.lock','a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 p=r/'private/process.json'
 def alive(d):
  try:
   stat=Path('/proc/'+str(d['pid'])+'/stat').read_text().split()
   cmd=Path('/proc/'+str(d['pid'])+'/cmdline').read_bytes().split(b'\\0')
   return stat[21]==d['start'] and str(r/'bin/frpc').encode() in cmd
  except (OSError,KeyError):return False
 d=json.loads(p.read_text()) if p.exists() else None
 if sys.argv[1]=='stop':
  if d and alive(d):os.kill(d['pid'],15);print('FRPC stop requested.')
  else:print('This bundle is not running.')
 elif d and alive(d):print('This bundle is already running.')
 else:
  subprocess.run([str(r/'bin/frpc'),'verify','-c',str(r/'config/frpc.toml')],check=True)
  with open(r/'logs/startup.log','ab') as log:
   c=subprocess.Popen([str(r/'bin/frpc'),'-c',str(r/'config/frpc.toml')],stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
  time.sleep(1)
  if c.poll() is not None:raise SystemExit('FRPC exited; inspect logs/startup.log and logs/frpc.log.')
  p.write_text(json.dumps({'pid':c.pid,'start':Path('/proc/'+str(c.pid)+'/stat').read_text().split()[21]}))
  print('FRPC process started. Inspect logs/frpc.log for authenticated login and proxy registration; startup alone is not platform acceptance.')
'''
    files={'bin/frpc':(binary,0o700),'config/frpc.toml':(toml.encode(),0o600),'certs/ca.crt':(cert,0o600),'private/transport-token':(transport,0o600),'private/token':(token.encode(),0o600),'manage.py':(manager.encode(),0o700)}
    for action in ['start','stop']:
        files[action+'.sh']=(f'#!/bin/sh\nset -eu\ncd "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\numask 077\nexec python3 ./manage.py {action}\n'.encode(),0o700)
    readme=f'''# Provider bundle for {launch}

Linux x86_64 / FRPC 0.71.1. Requires Python3 and Linux /proc. Keep this whole archive private: it contains transport and project credentials.

Project: {pid}
Provider: {provider}
Launch: {launch}
Original RPC deadline (Unix): {project['deadline']}

Extract into a private provider directory (umask 077), then run ./start.sh. The connector forwards to 127.0.0.1:8000 on THIS machine. Keep your model API running there. Use ./stop.sh to stop only this bundle. Do not edit the assigned identity, route or certificate settings.

The inference API key is separate and is NOT included. Login/proxy registration is not RPC/model/capacity acceptance. Check logs/frpc.log and the project setup page; no credit sales are opened by this download. The original stake deadline is never reset. Automatic reboot startup is not installed.

Bundled frpc is from https://github.com/fatedier/frp/releases/tag/v0.71.1 . Binary SHA256: {config['frpc_sha256']}. Upstream license is included in LICENSE.frp.
'''
    files['README.md']=(readme.encode(),0o600);files['LICENSE.frp']=(Path(config['license_file']).read_bytes(),0o600)
    manifest={'project_id':pid,'launch_id':launch,'provider_id':provider,'deadline':project['deadline'],'frp_version':'0.71.1','platform':'linux-amd64','files':{name:hashlib.sha256(data).hexdigest() for name,(data,_) in files.items()}}
    files['manifest.json']=(json.dumps(manifest,indent=2).encode(),0o600)
    stream=io.BytesIO();prefix='provider-'+launch
    with tarfile.open(fileobj=stream,mode='w:gz') as archive:
        for directory in ['','bin','config','certs','private','logs']:
            entry=tarfile.TarInfo(prefix+('/'+directory if directory else ''));entry.type=tarfile.DIRTYPE;entry.mode=0o700;archive.addfile(entry)
        for name,(data,mode) in files.items():
            entry=tarfile.TarInfo(prefix+'/'+name);entry.size=len(data);entry.mode=mode;archive.addfile(entry,io.BytesIO(data))
    return {'filename':prefix+'-linux-amd64.tar.gz','credential_sha256':hashlib.sha256(token.encode()).hexdigest(),'bundle_base64':base64.b64encode(stream.getvalue()).decode()}
if __name__=='__main__':
    import sys
    result=build(json.loads(Path(os.environ['PROVIDER_BUNDLE_CONFIG']).read_text()),json.load(sys.stdin));sys.stdout.write(json.dumps(result))
