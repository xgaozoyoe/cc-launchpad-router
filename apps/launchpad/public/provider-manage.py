"""Local lifecycle commands; no installation token or platform login required."""
import argparse,fcntl,json,os,re,socket,subprocess,sys,time,urllib.request
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent

def main():
    c=json.loads((ROOT/'.acc-install/runtime.json').read_text())
    connector=ROOT/c['connector']; model=ROOT/'model'
    parser=argparse.ArgumentParser()
    parser.add_argument('action',choices=['run','stop','status','logs']);parser.add_argument('log_target',nargs='?',choices=['model'])
    parser.add_argument('--gpu');parser.add_argument('--port',type=int)
    a=parser.parse_args();action=a.action
    if action!='run' and (a.gpu is not None or a.port is not None):parser.error('--gpu and --port are run options')
    lock=open(ROOT/'.acc-install/lifecycle.lock','a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    gpu=a.gpu if a.gpu is not None else c.get('gpu','0')
    port=a.port if a.port is not None else c.get('port',8000)
    if not re.fullmatch(r'[0-9]+',gpu):raise RuntimeError('GPU must be a single NVIDIA GPU index, e.g. --gpu 0.')
    if not 1024<=port<=65535:raise RuntimeError('Port must be between 1024 and 65535.')
    env=dict(os.environ,PORT=str(port),GPU_DEVICE=gpu,BIND_ADDRESS='127.0.0.1')
    def compose(*args):
        subprocess.run(['docker','compose','-p',c['compose_project'],*args],cwd=model,check=True,env=env)
    def ready():
        if c['managed_model']:
            key=dict(line.split('=',1) for line in (model/'.env').read_text().splitlines() if '=' in line)['INFERENCE_API_KEY']
        else:key=Path(c['api_key_file']).read_text().strip()
        req=urllib.request.Request(f'http://127.0.0.1:{port}/v1/models',headers={'Authorization':'Bearer '+key})
        with urllib.request.urlopen(req,timeout=5) as response:
            return any(m.get('id')=='qwen3-8b-awq' for m in json.load(response).get('data',[]))
    if action=='run':
        if c['managed_model']:
            subprocess.run(['nvidia-smi','-i',gpu,'--query-gpu=name','--format=csv,noheader'],check=True)
            # A port already owned by this Compose service is safe to reuse.
            probe=subprocess.run(['docker','compose','-p',c['compose_project'],'port','inference','8000'],cwd=model,env=env,capture_output=True,text=True)
            owned=probe.returncode==0 and probe.stdout.strip()==f'127.0.0.1:{port}'
            if not owned:
                with socket.socket() as sock:
                    try:sock.bind(('127.0.0.1',port))
                    except OSError:raise RuntimeError(f'Port {port} is occupied. Choose another --port.')
        elif a.gpu is not None:raise RuntimeError('Existing API mode does not manage GPUs; omit --gpu.')
        changed=c.get('port',8000)!=port or c.get('gpu','0')!=gpu
        if changed:subprocess.run(['sh',str(connector/'stop.sh')],check=True)
        config=connector/'config/frpc.toml'
        text=config.read_text();text,n=re.subn(r'(?m)^localPort\s*=\s*\d+\s*$',f'localPort = {port}',text)
        if n!=1:raise RuntimeError('Expected exactly one FRPC localPort setting.')
        config.write_text(text)
        c.update(gpu=gpu,port=port)
        (ROOT/'.acc-install/runtime.json').write_text(json.dumps(c))
        if c['managed_model']:compose('up','--build','-d')
        else:print('Using existing API; its lifecycle is managed separately.',flush=True)
        deadline=time.monotonic()+1800
        while True:
            try:
                if ready():break
            except Exception:pass
            if time.monotonic()>deadline:raise RuntimeError('API not ready after 30 minutes. Run ./logs.sh model. FRPC was not started.')
            print('Waiting for model API…',flush=True);time.sleep(10)
        subprocess.run(['sh',str(connector/'start.sh')],check=True)
        print('Model API ready. FRPC start checked; use ./status.sh and platform Test RPC to verify the tunnel.')
    elif action=='stop':
        subprocess.run(['sh',str(connector/'stop.sh')],check=True)
        if c['managed_model']:compose('stop')
        else:print('Existing API was left running.')
    elif action=='status':
        if c['managed_model']:compose('ps')
        try:print('Model API:', 'ready' if ready() else 'wrong model')
        except Exception:print('Model API: unavailable or authentication failed')
        try:
            d=json.loads((connector/'private/process.json').read_text());proc=Path('/proc')/str(d['pid'])
            alive=(proc/'stat').read_text().split()[21]==d['start'] and str(connector/'bin/frpc').encode() in (proc/'cmdline').read_bytes().split(b'\0')
        except (OSError,KeyError,ValueError):alive=False
        print('FRPC process:', 'running (does not prove tunnel health)' if alive else 'not running')
        print('Use platform Test RPC to check end-to-end connectivity.')
    elif action=='logs':
        if a.log_target=='model':
            if c['managed_model']:compose('logs','--tail','100','-f')
            else:print('Existing API logs are managed separately.')
        else:
            files=[str(p) for p in [connector/'logs/frpc.log',connector/'logs/startup.log'] if p.exists()]
            if files:subprocess.run(['tail','-n','100','-f',*files],check=True)
            else:print('No connector logs yet. Run ./run.sh first.')
    else:raise RuntimeError('Use run, status, logs [model], or stop.')
if __name__=='__main__':
    try:main()
    except (RuntimeError,OSError,ValueError,KeyError,subprocess.CalledProcessError) as e:raise SystemExit('Command failed: '+str(e))
