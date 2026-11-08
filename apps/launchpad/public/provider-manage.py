"""Local lifecycle commands; no installation token or platform login required."""
import json,os,subprocess,sys,time,urllib.request
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent

def main():
    c=json.loads((ROOT/'.acc-install/runtime.json').read_text())
    connector=ROOT/c['connector']; model=ROOT/'model'
    action=sys.argv[1]
    def compose(*args):
        subprocess.run(['docker','compose','-p',c['compose_project'],*args],cwd=model,check=True)
    def ready():
        if c['managed_model']:
            key=dict(line.split('=',1) for line in (model/'.env').read_text().splitlines() if '=' in line)['INFERENCE_API_KEY']
        else:key=Path(c['api_key_file']).read_text().strip()
        req=urllib.request.Request('http://127.0.0.1:8000/v1/models',headers={'Authorization':'Bearer '+key})
        with urllib.request.urlopen(req,timeout=5) as response:
            return any(m.get('id')=='qwen3-8b-awq' for m in json.load(response).get('data',[]))
    if action=='run':
        if c['managed_model']:compose('up','-d')
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
        if len(sys.argv)>2 and sys.argv[2]=='model':
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
