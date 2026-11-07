"""Finalized EVM stake monitor. No signing keys; JSON-RPC reads only."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import tempfile
import time
import urllib.request
import importlib.util

spec = importlib.util.spec_from_file_location('stake_lifecycle', Path(__file__).with_name('lifecycle.py'))
lifecycle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lifecycle)
STAKE_TOPIC = '0xbbf10c4c1d319626e97e16ffe46187dba6545801716d3e5268f5d783214be8c4'
STATE_SELECTOR = '0x45b69b1d'


def project_key(project_id):
    return '0x' + hashlib.sha256(project_id.encode()).hexdigest()


def atomic_private(path, body):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, temporary = tempfile.mkstemp(dir=path.parent)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, 'w') as output:
            output.write(body)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


class RPC:
    def __init__(self, url):
        self.url = url
    def call(self, method, params):
        request = urllib.request.Request(self.url, data=json.dumps(
            {'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),
            headers={'Content-Type':'application/json'})
        with urllib.request.urlopen(request, timeout=30) as response:
            result = json.load(response)
        if 'error' in result or 'result' not in result:
            raise ValueError('Chain RPC failed: ' + method)  # Do not log URL credentials or upstream pages.
        return result['result']


class StakeVerifier:
    require_contract_acceptance = True
    def __init__(self, rpc, config):
        self.rpc, self.config = rpc, config
        if type(config.get('chain_id')) is not int or config['chain_id'] <= 0:
            raise ValueError('Configure the platform default chain ID explicitly')
        import re
        if not re.fullmatch(r'0x[0-9a-fA-F]{40}', config.get('staking_contract','')):
            raise ValueError('Configure the deployed staking contract')
        if not re.fullmatch(r'0x[0-9a-fA-F]{40}', config.get('token_address','')):
            raise ValueError('Configure the actual default-chain PRL token address')
        self.contract = config['staking_contract'].lower()
    def finalized(self):
        if int(self.rpc.call('eth_chainId', []),16) != self.config['chain_id']:
            raise ValueError('RPC is not the configured platform default chain')
        token=self.rpc.call('eth_call',[{'to':self.contract,'data':'0x51ed6a30'},'latest'])
        if len(token)!=66 or ('0x'+token[-40:]).lower()!=self.config['token_address'].lower():
            raise ValueError('Stake contract uses a different token')
        block = self.rpc.call('eth_getBlockByNumber', ['finalized',False])
        if not block:
            raise ValueError('RPC must support finalized blocks; no latest-block fallback')
        return block
    def state(self, key, block='latest'):
        value = self.rpc.call('eth_call',[{'to':self.contract,'data':STATE_SELECTOR+key[2:]},block])
        words = [int(value[i:i+64],16) for i in range(2,len(value),64)]
        if len(words)!=5:
            raise ValueError('Unexpected staking contract state ABI')
        return {'status':words[0], 'amount':words[1], 'at':words[2], 'deadline':words[3], 'owner':'0x'+format(words[4],'040x')}
    def contract_verified(self, project):
        final = self.finalized()
        return self.state(project_key(project['id']),final['number'])['status']==3
    def __call__(self, tx_hash, project, chain):
        if chain['chain_id'] != self.config['chain_id'] or chain['staking_contract'].lower()!=self.contract:
            raise ValueError('Default chain configuration mismatch')
        final = self.finalized()
        receipt = self.rpc.call('eth_getTransactionReceipt',[tx_hash])
        if not receipt or receipt.get('status')!='0x1' or receipt.get('to','').lower()!=self.contract:
            raise ValueError('Stake transaction is missing, reverted, or sent to another contract')
        if receipt.get('transactionHash','').lower()!=tx_hash.lower():
            raise ValueError('Receipt transaction hash mismatch')
        if int(receipt['blockNumber'],16)>int(final['number'],16):
            raise ValueError('Stake transaction is not finalized')
        block = self.rpc.call('eth_getBlockByNumber',[receipt['blockNumber'],False])
        if not block or block['hash'].lower()!=receipt['blockHash'].lower():
            raise ValueError('Stake receipt is not canonical')
        matches=[log for log in receipt['logs'] if log.get('address','').lower()==self.contract
                 and str(log.get('topics',[None])[0]).lower()==STAKE_TOPIC and not log.get('removed')]
        if len(matches)!=1:
            raise ValueError('Require exactly one ProjectStaked event per transaction')
        event=matches[0]
        topics=event['topics']; raw=event['data']
        if len(topics)!=3 or topics[1].lower()!=project_key(project['id']) or len(raw)!=194:
            raise ValueError('Staking event project or ABI mismatch')
        owner='0x'+topics[2][-40:].lower()
        amount, at, deadline=(int(raw[i:i+64],16) for i in (2,66,130))
        if (owner!=project['owner'].lower() or receipt.get('from','').lower()!=owner
                or amount!=project['stake_required'] or at!=int(block['timestamp'],16) or deadline!=at+86400):
            raise ValueError('Staking owner, amount, or block deadline mismatch')
        for tag in (final['number'], 'latest'):
            current=self.state(project_key(project['id']),tag)
            if current!={'status':current['status'],'amount':amount,'at':at,'deadline':deadline,'owner':owner} or current['status'] not in (2,3):
                raise ValueError('Stake no longer active or contract state mismatch')
        return {'chain_id':self.config['chain_id'],'contract':self.contract,'tx_hash':tx_hash.lower(),
                'project_id':project['id'],'owner':owner,'amount':amount,'block_timestamp':at,'block_hash':block['hash'].lower()}


class Monitor:
    def __init__(self, server_dir, config, rpc=None, clock=time.time):
        self.root=Path(server_dir)
        self.root.mkdir(parents=True,exist_ok=True,mode=0o700)
        if type(config.get('deployment_block')) is not int or config['deployment_block'] < 0:
            raise ValueError('Configure deployment block')
        if type(config.get('batch_size',100)) is not int or not 1 <= config.get('batch_size',100) <= 1000:
            raise ValueError('Batch size must be 1–1000 blocks')
        self.config=config; self.clock=clock
        self.verifier=StakeVerifier(rpc or RPC(os.environ['CHAIN_RPC_URL']),config)
        self.db=sqlite3.connect(self.root/'monitor.sqlite',isolation_level=None)
        os.chmod(self.root/'monitor.sqlite',0o600)
        self.db.executescript('''CREATE TABLE IF NOT EXISTS snapshot(id INTEGER PRIMARY KEY CHECK(id=1),data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS cursor(id INTEGER PRIMARY KEY CHECK(id=1),height INTEGER,hash TEXT);
          CREATE TABLE IF NOT EXISTS events(tx TEXT PRIMARY KEY,project TEXT,outcome TEXT);
          CREATE TABLE IF NOT EXISTS deliveries(launch TEXT PRIMARY KEY,payload TEXT NOT NULL);''')
        identity={'chain_id':config['chain_id'],'staking_contract':config['staking_contract'].lower(),'token_address':config['token_address'].lower()}
        row=self.db.execute('SELECT data FROM snapshot WHERE id=1').fetchone()
        if not row:
            data=json.loads((self.root/'registry.json').read_text())
            data.setdefault('projects',{}); data['default_chain']=identity
            self.db.execute('INSERT INTO snapshot VALUES(1,?)',(json.dumps(data),))
        elif json.loads(row[0]).get('default_chain')!=identity:
            raise ValueError('Changing the registry default chain/contract requires a new audited monitor database')
        self.export()  # Recover durable outbox after a crash between DB commit and file export.
    def data(self):
        return json.loads(self.db.execute('SELECT data FROM snapshot WHERE id=1').fetchone()[0])
    def save(self,data):
        self.db.execute('UPDATE snapshot SET data=? WHERE id=1',(json.dumps(data),))
    def create_project(self,owner,name,profile,amount):
        import re
        if not re.fullmatch(r'0x[0-9a-fA-F]{40}',owner):
            raise ValueError('Owner must be an EVM address')
        self.db.execute('BEGIN IMMEDIATE')
        try:
            data=self.data()
            project=lifecycle.Lifecycle(data).create(owner.lower(),name,profile,amount)
            self.save(data);self.db.execute('COMMIT')
        except Exception:
            self.db.execute('ROLLBACK');raise
        self.export()
        return project
    def bundle(self,data,p,credential):
        provider=data['providers'][p['provider_id']]
        config=f'''serverAddr = {json.dumps(data['server'])}
serverPort = 7000
user = {json.dumps(p['provider_id'])}
loginFailExit = false
metadatas.provider_token = {json.dumps(credential)}
auth.method = "token"
auth.tokenSource.type = "file"
auth.tokenSource.file.path = "/etc/frp/token"
auth.additionalScopes = ["HeartBeats", "NewWorkConns"]
transport.tls.enable = true
transport.tls.trustedCaFile = "/etc/frp/ca.crt"
transport.tls.serverName = {json.dumps(data['server'])}
log.level = "warn"
[[proxies]]
name = "inference"
type = "http"
localIP = "inference"
localPort = 8000
subdomain = {json.dumps(provider['tunnel_id'])}
'''
        return {'frpc.toml':config,'ca.crt':(self.root/'ca.crt').read_text(),'token':(self.root/'token').read_text(),
                'launch.json':json.dumps({'project_id':p['id'],'launch_id':p['launch_id'],'provider_id':p['provider_id'],
                    'endpoint':data['launches'][p['launch_id']]['endpoint'],'verification_deadline':p['verification_deadline']},indent=2)}
    def export(self):
        atomic_private(self.root/'registry.json',json.dumps(self.data(),indent=2)+'\n')
        for launch,payload in self.db.execute('SELECT launch,payload FROM deliveries'):
            for name,body in json.loads(payload).items():
                atomic_private(self.root/'deliveries'/launch/name,body)
    def accept_rpc(self,project_id,probe):
        # Invoke from an authenticated backend only, with an actual trusted probe.
        self.db.execute('BEGIN IMMEDIATE')
        try:
            data=self.data()
            result=lifecycle.Lifecycle(data,data['default_chain'],self.verifier,self.clock).accept_rpc(project_id,probe)
            self.save(data);self.db.execute('COMMIT')
        except Exception:
            self.db.execute('ROLLBACK');raise
        self.export()
        return result

    def reconcile(self):
        data=self.data()
        changed=False
        for p in data['projects'].values():
            if not p.get('provider_id'):continue
            state=self.verifier.state(project_key(p['id']))
            if state['status'] not in (2,3):
                p['status']='refunded' if state['status']==4 else 'released' if state['status']==5 else 'stake-invalid'
                data['providers'][p['provider_id']]['enabled']=False
                data['launches'][p['launch_id']]['enabled']=False
                changed=True
            elif p['status']=='awaiting-rpc' and int(self.clock())>=p['verification_deadline']:
                p['status']='expired'
                data['providers'][p['provider_id']]['enabled']=False
                data['launches'][p['launch_id']]['enabled']=False
                changed=True
        if changed:self.save(data);self.export()

    def tick(self):
        final=self.verifier.finalized()
        cursor=self.db.execute('SELECT height,hash FROM cursor WHERE id=1').fetchone()
        if cursor:
            canonical=self.verifier.rpc.call('eth_getBlockByNumber',[hex(cursor[0]),False])
            if not canonical or canonical['hash'].lower()!=cursor[1].lower():
                # Freeze all monitored routes before stopping. Never silently follow a finalized reorg.
                data=self.data()
                for item in data['providers'].values():item['enabled']=False
                for item in data['launches'].values():item['enabled']=False
                self.save(data);self.export()
                raise ValueError('Finalized checkpoint changed; all routes disabled for operator reconciliation')
        start=cursor[0]+1 if cursor else self.config['deployment_block']
        stop=min(int(final['number'],16),start+self.config.get('batch_size',100)-1)
        self.reconcile()
        if start>stop:return 0
        logs=self.verifier.rpc.call('eth_getLogs',[{'address':self.verifier.contract,'topics':[STAKE_TOPIC],
                            'fromBlock':hex(start),'toBlock':hex(stop)}])
        accepted=0
        self.db.execute('BEGIN IMMEDIATE')
        try:
            data=self.data()
            projects={project_key(p['id']):p for p in data['projects'].values()}
            for log in sorted(logs,key=lambda log:(int(log['blockNumber'],16),int(log['logIndex'],16))):
                tx=log['transactionHash'].lower()
                if self.db.execute('SELECT 1 FROM events WHERE tx=?',(tx,)).fetchone():continue
                p=projects.get(log.get('topics',[None,None])[1].lower())
                if not p:
                    # Do not checkpoint past unknown projects: synchronize project creation, then retry.
                    raise ValueError('Stake event has no backend project; synchronize before advancing cursor')
                state=self.verifier.state(project_key(p['id']))
                if state['status'] in (4,5) or (state['at']>0 and int(self.clock())>=state['deadline'] and state['status']==2):
                    p['status']='refunded' if state['status']==4 else 'released' if state['status']==5 else 'expired'
                    self.db.execute('INSERT INTO events VALUES(?,?,?)',(tx,p['id'],p['status']))
                    continue
                try:
                    flow=lifecycle.Lifecycle(data,data['default_chain'],self.verifier,self.clock)
                    p,credential=flow.confirm_stake(p['id'],tx)
                except ValueError:
                    # Transient RPC errors must retry, not become terminal rejections.
                    raise
                if credential:
                    self.db.execute('INSERT INTO deliveries VALUES(?,?)',(p['launch_id'],json.dumps(self.bundle(data,p,credential))))
                self.db.execute('INSERT INTO events VALUES(?,?,?)',(tx,p['id'],'issued'))
                accepted+=1
            anchor=self.verifier.rpc.call('eth_getBlockByNumber',[final['number'],False])
            if not anchor or anchor['hash'].lower()!=final['hash'].lower():
                raise ValueError('Finalized scan anchor changed during processing; nothing committed')
            checkpoint=self.verifier.rpc.call('eth_getBlockByNumber',[hex(stop),False])
            self.save(data)
            self.db.execute('INSERT OR REPLACE INTO cursor VALUES(1,?,?)',(stop,checkpoint['hash'].lower()))
            self.db.execute('COMMIT')
        except Exception:
            self.db.execute('ROLLBACK');raise
        self.export()
        return accepted
