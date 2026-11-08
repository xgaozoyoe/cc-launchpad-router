"""Project-scoped Qwen/FRPC installer. Run in the desired provider directory."""
import fcntl,argparse,hashlib,io,json,os,platform,secrets,shutil,socket,subprocess,tarfile,time,urllib.request,urllib.error
from pathlib import Path
BASE='https://riddle.xyz'
def unpack(data,dest):
    with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as archive:
        for m in archive.getmembers():
            target=(dest/m.name).resolve()
            if not target.is_relative_to(dest.resolve()) or not (m.isfile() or m.isdir()):raise RuntimeError('Invalid archive entry')
        archive.extractall(dest)
def main():
    p=argparse.ArgumentParser();p.add_argument('--launch-id',required=True);p.add_argument('--token',required=True);p.add_argument('--existing-api-key-file');p.add_argument('--wait-seconds',type=int,default=1800);a=p.parse_args()
    if platform.system()!='Linux' or platform.machine() not in ('x86_64','amd64'):raise RuntimeError('Requires Linux x86_64.')
    os.umask(0o077);root=Path.cwd();work=root/'.acc-install';work.mkdir(exist_ok=True)
    lock=open(work/'install.lock','a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    if not a.existing_api_key_file:
        for executable in ['docker','nvidia-smi']:
            if not shutil.which(executable):raise RuntimeError(executable+' is required. Install Docker and NVIDIA Container Toolkit before retrying.')
        subprocess.run(['docker','compose','version'],check=True);subprocess.run(['docker','info'],check=True,stdout=subprocess.DEVNULL)
        subprocess.run(['nvidia-smi','--query-gpu=name,memory.total','--format=csv'],check=True)
    def api(action,**values):
        req=urllib.request.Request(BASE+'/api/provider-install/'+action,data=json.dumps({'launch_id':a.launch_id,**values}).encode(),headers={'Authorization':'Bearer '+a.token,'Content-Type':'application/json'})
        try:
            with urllib.request.urlopen(req,timeout=90) as response:return response.read()
        except urllib.error.HTTPError as e:
            try:message=json.load(e).get('error','Request failed')
            except Exception:message='Request failed (HTTP '+str(e.code)+')'
            raise RuntimeError(message) from None
    print('1/4 Downloading assigned connector…',flush=True)
    bundle=api('bundle');unpack(bundle,root);connector=root/('provider-'+a.launch_id)
    manifest=json.loads((connector/'manifest.json').read_text())
    if manifest['launch_id']!=a.launch_id:raise RuntimeError('Bundle identity mismatch')
    for name,digest in manifest['files'].items():
        if hashlib.sha256((connector/name).read_bytes()).hexdigest()!=digest:raise RuntimeError('Bundle checksum mismatch')
    # Generate reusable commands before model startup, so failed startup is recoverable.
    with urllib.request.urlopen(BASE+'/provider-manage.py',timeout=60) as response:
        (work/'manage.py').write_bytes(response.read())
    project='acc-'+hashlib.sha256(str(root).encode()).hexdigest()[:12]
    (work/'runtime.json').write_text(json.dumps({'connector':connector.name,'compose_project':project,'managed_model':not bool(a.existing_api_key_file),'api_key_file':str(Path(a.existing_api_key_file).resolve()) if a.existing_api_key_file else None}))
    for name,action in [('run','run'),('status','status'),('logs','logs'),('stop','stop')]:
        script=root/(name+'.sh')
        script.write_text('#!/bin/sh\nset -eu\ncd "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"\nexec python3 .acc-install/manage.py '+action+' "$@"\n')
        script.chmod(0o700)
    print('Local commands created: ./run.sh, ./status.sh, ./logs.sh [model], ./stop.sh',flush=True)
    print('2/4 Preparing model API…',flush=True)
    if a.existing_api_key_file:key=Path(a.existing_api_key_file).read_text().strip()
    else:
        model=root/'model';model.mkdir(exist_ok=True)
        with urllib.request.urlopen(BASE+'/provider-runtime.tar.gz',timeout=60) as response:unpack(response.read(),model)
        env=model/'.env'
        if not env.exists():env.write_text('INFERENCE_API_KEY='+secrets.token_urlsafe(32)+'\n')
        key=dict(line.split('=',1) for line in env.read_text().splitlines() if '=' in line)['INFERENCE_API_KEY']
        project='acc-'+hashlib.sha256(str(root).encode()).hexdigest()[:12]
        print('Model commands: cd model && docker compose -p '+project+' logs / stop',flush=True)
        subprocess.run(['docker','compose','-p',project,'up','--build','-d'],cwd=model,check=True)
    deadline=time.monotonic()+a.wait_seconds
    while True:
        try:
            req=urllib.request.Request('http://127.0.0.1:8000/v1/models',headers={'Authorization':'Bearer '+key})
            with urllib.request.urlopen(req,timeout=5) as response:models=json.load(response)
            if not any(m.get('id')=='qwen3-8b-awq' for m in models.get('data',[])):raise RuntimeError('API does not serve qwen3-8b-awq')
            break
        except (urllib.error.URLError,TimeoutError):
            if time.monotonic()>deadline:raise RuntimeError('Model still starting. Inspect docker compose logs in model/; rerun to continue.')
            print('Waiting for model download / startup…',flush=True);time.sleep(15)
    print('3/4 Starting FRPC…',flush=True);subprocess.run(['sh',str(connector/'start.sh')],check=True)
    time.sleep(5)
    print('4/4 Testing API through the platform…',flush=True)
    result=json.loads(api('test',inference_api_key=key));(work/'rpc-result.json').write_text(json.dumps(result,indent=2))
    report=result.get('report') or {};print('Platform API test:',report.get('status','running'))
    if report.get('failure'):print(report['failure'])
    print('Full test results: .acc-install/rpc-result.json\nConnector logs: '+str(connector/'logs/frpc.log'))
    print('Return to Provider setup to review results and authorize buyer usage. This installer does not approve sales.')
    if report.get('status')=='failed':raise RuntimeError('Platform test failed; inspect the saved report and rerun after correction.')
if __name__=='__main__':
    try:main()
    except (RuntimeError,subprocess.CalledProcessError,KeyError) as e:raise SystemExit('Setup stopped: '+str(e))
