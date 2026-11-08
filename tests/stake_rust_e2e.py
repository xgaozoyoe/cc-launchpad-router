"""Explicit integration test: disposable Anvil + authenticated MongoDB + real Rust API.
Run with an eth-account Python environment after cargo build and forge build.
"""
import base64,hashlib,hmac,json,os,secrets,socket,subprocess,tempfile,time,urllib.request,urllib.error,uuid
from pathlib import Path
from eth_account import Account
from eth_account.messages import encode_defunct
from eth_utils import keccak
from eth_abi import encode
ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('CARGO_TARGET_DIR','/tmp/cc-central-target'))/'debug'
FOUNDRY=Path.home()/'.foundry/bin'
def free_port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
def wait(fn,timeout=60):
 end=time.monotonic()+timeout
 while time.monotonic()<end:
  try:
   value=fn()
   if value:return value
  except (OSError,urllib.error.URLError):pass
  time.sleep(.5)
 raise AssertionError('Timed out waiting for integration state')
def project_key_for_test(id):return hashlib.sha256(id.encode()).hexdigest()
def run():
 children=[]
 with tempfile.TemporaryDirectory(prefix='stake-rust-') as temp:
  tmp=Path(temp);log=open(tmp/'service.log','w+');os.chmod(tmp,0o700)
  def start(cmd,env=None):
   p=subprocess.Popen(cmd,cwd=ROOT,env=env,stdout=log,stderr=log);children.append(p);return p
  chain_port,mongo_port,api_port=free_port(),free_port(),free_port()
  rpc_url=f'http://127.0.0.1:{chain_port}';api_url=f'http://127.0.0.1:{api_port}'
  def rpc(method,params):
   req=urllib.request.Request(rpc_url,data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),headers={'Content-Type':'application/json'})
   with urllib.request.urlopen(req,timeout=10) as r:d=json.load(r)
   if 'error' in d:raise AssertionError(d['error'])
   return d['result']
  def send(sender,to,data,value=0):
   tx={'from':sender,'data':data,'gas':'0x1400000','value':hex(value)}
   if to:tx['to']=to
   h=rpc('eth_sendTransaction',[tx]);r=wait(lambda:rpc('eth_getTransactionReceipt',[h]));assert r['status']=='0x1';return r
  selector=lambda s:keccak(text=s)[:4].hex()
  word=lambda n:f'{n:064x}'
  addr=lambda a:a[2:].lower().rjust(64,'0')
  key=bytes([7])*32
  def api(method,path,body=None,user='alice',expected=200):
   raw=b'' if body is None else json.dumps(body).encode();stamp=str(int(time.time()));nonce=secrets.token_hex(16)
   message='\n'.join((method,path,user,stamp,nonce,hashlib.sha256(raw).hexdigest()))
   headers={'Content-Type':'application/json','X-CC-User':user,'X-CC-Timestamp':stamp,'X-CC-Nonce':nonce,'X-CC-Signature':hmac.new(key,message.encode(),hashlib.sha256).hexdigest()}
   request=urllib.request.Request(api_url+path,data=raw if method=='POST' else None,headers=headers,method=method)
   try:
    with urllib.request.urlopen(request,timeout=35) as r:code,data=r.status,json.load(r)
   except urllib.error.HTTPError as e:code,data=e.code,json.load(e)
   assert code==expected,(path,code,data)
   return data
  try:
   start([str(FOUNDRY/'anvil'),'--host','127.0.0.1','--port',str(chain_port),'--silent','--cache-path',str(tmp/'anvil-cache'),'--prune-history','100000'])
   admin=wait(lambda:rpc('eth_accounts',[]))[0]
   token_bytecode=json.loads((ROOT/'contracts/out/TestPRL.sol/TestPRL.json').read_text())['bytecode']['object']
   progressive=os.environ.get('PROGRESSIVE_STAKE_TEST')=='1'
   stake_artifact='LaunchStakeV2' if progressive else 'LaunchStake'
   stake_bytecode=json.loads((ROOT/f'contracts/out/{stake_artifact}.sol/{stake_artifact}.json').read_text())['bytecode']['object']
   token_receipt=send(admin,None,token_bytecode+addr(admin)+word(1000000*10**18));token=token_receipt['contractAddress']
   escrow=send(admin,None,stake_bytecode+addr(token)+addr(admin))['contractAddress']
   settler=Account.create();rpc('anvil_setBalance',[settler.address,hex(10**18)])
   credits_artifact='ComputeCreditsV2' if progressive else 'ComputeCredits'
   credits_code=json.loads((ROOT/f'contracts/out/{credits_artifact}.sol/{credits_artifact}.json').read_text())['bytecode']['object']
   credit_receipt=send(admin,None,credits_code+addr(escrow)+addr(admin)+addr(settler.address)+addr(admin));credits=credit_receipt['contractAddress']
   if progressive:send(admin,escrow,'0x'+selector('bindCreditContract(address)')+addr(credits))
   (tmp/'settler.json').write_text(json.dumps(Account.encrypt(settler.key,'test-password')))
   (tmp/'credits.json').write_text(json.dumps({'chain_id':31337,'contract':credits,'staking_contract':escrow,'settler':settler.address,'deployment_block':int(credit_receipt['blockNumber'],16)}))
   provider=Account.create();rpc('anvil_setBalance',[provider.address,hex(10**18)]);rpc('anvil_impersonateAccount',[provider.address])
   send(admin,token,'0x'+selector('transfer(address,uint256)')+addr(provider.address)+word(2000*10**18))
   assert int(rpc('eth_chainId',[]),16)==31337
   # Standard public Anvil development key; never use it for a funded public-chain account.
   admin_key='ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80'
   (tmp/'operator.json').write_text(json.dumps(Account.encrypt(bytes.fromhex(admin_key),'test-password')));(tmp/'password').write_text('test-password\n')
   (tmp/'hmac').write_text(key.hex());(tmp/'mongo-key').write_text(base64.b64encode(secrets.token_bytes(512)).decode());(tmp/'mongo-key').chmod(0o600)
   (tmp/'db').mkdir()
   mongod=os.environ.get('MONGOD','/opt/homebrew/opt/mongodb-community@5.0/bin/mongod')
   start([mongod,'--dbpath',str(tmp/'db'),'--port',str(mongo_port),'--bind_ip','127.0.0.1','--replSet','rs0','--auth','--keyFile',str(tmp/'mongo-key')])
   root_pw,app_pw=secrets.token_hex(32),secrets.token_hex(32)
   env={**os.environ,'MONGO_PORT':str(mongo_port),'MONGO_ROOT_PASSWORD':root_pw,'MONGO_APP_PASSWORD':app_pw}
   time.sleep(1)
   subprocess.run([str(BIN/'bootstrap-mongo')],env=env,check=True,stdout=log,stderr=log)
   config={'chain_id':31337,'staking_contract':escrow,'token_address':token,'deployment_block':int(token_receipt['blockNumber'],16)}
   (tmp/'chain.json').write_text(json.dumps(config))
   env.update(MONGODB_URI=f'mongodb://launchpad:{app_pw}@127.0.0.1:{mongo_port}/launchpad?replicaSet=rs0&authSource=launchpad',CENTRAL_HMAC_KEY_FILE=str(tmp/'hmac'),BIND_ADDRESS=f'127.0.0.1:{api_port}',CHAIN_CONFIG_FILE=str(tmp/'chain.json'),CHAIN_RPC_URL=rpc_url,CHAIN_ALLOW_LOCAL='1',APP_ORIGIN='http://localhost:4174',STAKE_CAST=str(FOUNDRY/'cast'),STAKE_KEYSTORE=str(tmp/'operator.json'),STAKE_PASSWORD_FILE=str(tmp/'password'))
   (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[]}))
   env['PROTOCOL_GROUPS_FILE']=str(tmp/'protocol-groups.json')
   env.update(CREDIT_CONFIG_FILE=str(tmp/'credits.json'),CREDIT_SETTLER_KEYSTORE=str(tmp/'settler.json'),CREDIT_SETTLER_PASSWORD_FILE=str(tmp/'password'))
   (tmp/'frpc').write_bytes(b'test-binary');(tmp/'ca').write_text('test-cert');(tmp/'transport').write_text('transport-key');(tmp/'license').write_text('license')
   bundle_config={'python':os.environ.get('BUNDLE_PYTHON','python3'),'helper':str(ROOT/'scripts/provider_bundle.py'),'server_addr':'127.0.0.1','server_port':7000,'tls_server_name':'localhost','frpc_binary':str(tmp/'frpc'),'frpc_sha256':hashlib.sha256(b'test-binary').hexdigest(),'ca_file':str(tmp/'ca'),'transport_token_file':str(tmp/'transport'),'license_file':str(tmp/'license'),'credential_dir':str(tmp/'credentials')}
   (tmp/'bundles.json').write_text(json.dumps(bundle_config));env['PROVIDER_BUNDLE_CONFIG']=str(tmp/'bundles.json')
   from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
   import threading
   observed_hosts=[]
   trial_started=threading.Event()
   class Inference(BaseHTTPRequestHandler):
    def do_GET(self):
     observed_hosts.append(self.headers.get('Host'))
     if self.path=='/health':self.send_response(200);self.end_headers();return
     if self.headers.get('Authorization')!='Bearer test-inference-secret':self.send_response(401);self.end_headers();return
     self.send_response(200);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps({'data':[{'id':'qwen3-8b-awq'}]}).encode())
    def do_POST(self):
     body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
     if body.get('messages',[{}])[-1].get('content')=='slow-trial':trial_started.set();time.sleep(1)
     usage={'prompt_tokens':2,'completion_tokens':3,'total_tokens':5}
     self.send_response(200);self.send_header('Content-Type','text/event-stream' if body.get('stream') else 'application/json');self.end_headers()
     if body.get('stream'):
      chunks=[{'choices':[{'delta':{'content':'hello'}}]},{'choices':[],'usage':usage}]
      for chunk in chunks:self.wfile.write(('data: '+json.dumps(chunk)+'\n\n').encode());self.wfile.flush()
      self.wfile.write(b'data: [DONE]\n\n')
     else:self.wfile.write(json.dumps({'choices':[{'message':{'content':'hello'},'finish_reason':'stop'}],'usage':usage}).encode())
    def log_message(self,*_):pass
   inference=ThreadingHTTPServer(('127.0.0.1',0),Inference);threading.Thread(target=inference.serve_forever,daemon=True).start();env['RPC_CHECK_PORT']=str(inference.server_port);env['ADMIN_WALLET_ADDRESS']=admin;env['RPC_CREDENTIAL_DIR']=str(tmp/'rpc-credentials')
   backend=start([str(BIN/'cc-central')],env)
   wait(lambda:urllib.request.urlopen(api_url+'/healthz',timeout=1).status==200)
   legacy=str(uuid.uuid4())
   legacy_project=api('POST','/api/v1/projects',{'name':'Legacy login persistence','model_id':'qwen3-8b-awq','request_id':str(uuid.uuid4())},user=legacy)['project']
   api('POST','/api/v1/account/link-legacy',{'legacy_account_id':legacy},user='google:test-stable-subject')
   api('GET','/api/v1/projects/'+legacy_project['_id'],user='google:test-stable-subject')
   api('GET','/api/v1/projects/'+legacy_project['_id'],user=legacy)
   api('POST','/api/v1/account/link-legacy',{'legacy_account_id':legacy},user='google:another-subject',expected=409)
   project=api('POST','/api/v1/projects',{'name':'Rust stake integration','model_id':'qwen3-8b-awq','request_id':str(uuid.uuid4())})['project'];pid=project['_id'];path=f'/api/v1/projects/{pid}'
   assert not api('GET','/api/v1/launchpad',user='bob')['launches'] # private drafts never enter directory
   challenge=api('POST',path+'/stake/challenge',{'wallet_address':provider.address,'stake_amount':'2000'})
   wrong=Account.sign_message(encode_defunct(text=challenge['message']),Account.create().key).signature.hex()
   api('POST',path+'/stake/prepare',{'challenge_id':challenge['challenge_id'],'signature':'0x'+wrong.removeprefix('0x')},expected=400)
   signature=Account.sign_message(encode_defunct(text=challenge['message']),provider.key).signature.hex()
   proof={'challenge_id':challenge['challenge_id'],'signature':'0x'+signature.removeprefix('0x')}
   api('POST',path+'/stake/prepare',proof)
   api('POST',path+'/stake/prepare',proof,expected=400)
   api('GET',path,user='bob',expected=404)
   def get_status(status):
    p=api('GET',path)['project'];return p if p['status']==status else None
   registered=wait(lambda:get_status('awaiting-stake'),90);assert 'launch_id' not in registered
   details=api('GET',path+'/stake');assert details['token_balance_raw']==str(2000*10**18)
   approve=send(provider.address,token,details['approve_data'])
   api('POST',path+'/stake/submit',{'tx_hash':approve['transactionHash']},expected=400)
   stake_receipt=send(provider.address,escrow,details['stake_data']);tx_hash=stake_receipt['transactionHash']
   snapshot=rpc('evm_snapshot',[])
   api('POST',path+'/stake/submit',{'tx_hash':tx_hash})
   before=api('GET',path)['project'];assert 'launch_id' not in before
   rpc('anvil_mine',['0x80'])
   issued=wait(lambda:get_status('awaiting-rpc'),90)
   stake_block=rpc('eth_getBlockByNumber',[stake_receipt['blockNumber'],False]);at=int(stake_block['timestamp'],16)
   assert issued['provider_id']==tx_hash and issued['staked_at']==at and issued['rpc_deadline']==at+86400 and issued['stake_finalized'] is True
   directory=api('GET','/api/v1/launchpad',user='bob');public=next(p for p in directory['launches'] if p['_id']==pid)
   assert public['launch_id']==issued['launch_id'] and directory['sales_enabled'] is False
   assert set(public)<= {'_id','name','model_id','status','launch_id','provider_id','stake_raw','stake_finalized','capacity_approved','progress','sales_enabled','trial_available'}
   assert not public['capacity_approved'] and 'account_id' not in public
   import io,tarfile
   api('POST',path+'/deployment/bundle',{},user='bob',expected=404)
   bundle=api('POST',path+'/deployment/bundle',{})
   with tarfile.open(fileobj=io.BytesIO(base64.b64decode(bundle['bundle_base64'])),mode='r:gz') as archive:
    credential=archive.extractfile('provider-'+issued['launch_id']+'/private/token').read().decode()
   assert api('GET',path+'/deployment')['issued'] is True
   assert 'credential' not in str(api('GET',path))
   def hook(content):
    request=urllib.request.Request(api_url+'/internal/frp/handler?op=Login',data=json.dumps({'content':content}).encode(),headers={'Content-Type':'application/json'})
    return json.load(urllib.request.urlopen(request))
   login={'user':issued['provider_id'],'metas':{'provider_token':credential}}
   assert hook(login)['reject'] is False
   assert hook({'user':issued['provider_id'],'metas':{'provider_token':'wrong'}})['reject'] is True
   route='p-'+hashlib.sha256(issued['provider_id'].encode()).hexdigest()[:40]+'.launchpad.internal'
   request=urllib.request.Request(api_url+'/internal/frp/handler?op=NewProxy',data=json.dumps({'content':{'user':login,'proxy_type':'http','proxy_name':issued['provider_id']+'.inference','custom_domains':[route]}}).encode(),headers={'Content-Type':'application/json'})
   assert json.load(urllib.request.urlopen(request))['reject'] is False
   api('POST',path+'/deployment/rpc-check',{'inference_api_key':'test-inference-secret'},user='bob',expected=404)
   api('POST',path+'/deployment/rpc-check',{'inference_api_key':'test-inference-secret','base_url':'http://attacker'},expected=400)
   failed=api('POST',path+'/deployment/rpc-check',{'inference_api_key':'incorrect-inference-key'})
   assert failed['report']['status']=='failed' and 'incorrect-inference-key' not in str(failed)
   assert api('GET',path+'/deployment/rpc-check')['running'] is False
   time.sleep(11)
   checked=api('POST',path+'/deployment/rpc-check',{'inference_api_key':'test-inference-secret'})
   assert checked['report']['status']=='passed' and all(c['passed'] for c in checked['report']['checks'])
   assert checked['capacity_accepted'] is False and checked['sales_enabled'] is False
   assert 'test-inference-secret' not in str(api('GET',path+'/deployment/rpc-check'))
   assert observed_hosts and all(h==route for h in observed_hosts)
   detail=api('GET',path)['project'];assert detail['status']=='awaiting-rpc'
   assert detail['progress']['rpc_status']=='passed' and detail['progress']['last_proxy_at']>0
   listed=next(p for p in api('GET','/api/v1/projects')['projects'] if p['_id']==pid)
   assert listed['progress']==detail['progress']
   assert set(listed['progress'])=={'bundle_issued','last_proxy_at','rpc_status','rpc_checked_at'}
   assert all(p['_id']!=pid for p in api('GET','/api/v1/projects',user='bob')['projects'])

   # Every privileged operation needs a fresh signature from the configured wallet.
   def admin_sign(intent,payload,signer=admin_key,user='alice',expected=200):
    c=api('POST','/api/v1/admin/challenge',{'intent':intent,'payload':payload},user=user)
    sig=Account.sign_message(encode_defunct(text=c['message']),signer).signature.hex()
    proof={'challenge_id':c['challenge_id'],'signature':'0x'+sig.removeprefix('0x')}
    return api('POST','/api/v1/admin/'+('authenticate' if intent=='login' else intent),proof,user=user,expected=expected),proof
   api('POST','/api/v1/admin/query',{'token':'forged'},expected=403)
   admin_sign('login',{},provider.key,expected=403)
   session,proof=admin_sign('login',{})
   api('POST','/api/v1/admin/authenticate',proof,expected=403)
   listing=api('POST','/api/v1/admin/query',{'token':session['token']})
   assert any(p['_id']==pid for p in listing['projects'])
   api('POST','/api/v1/admin/query',{'token':session['token']},user='bob',expected=403)
   # Candidate audits must never activate contracts or change legacy routing.
   api('POST','/api/v1/admin/protocol',{'token':'forged'},expected=403)
   api('POST','/api/v1/admin/protocol',{'token':session['token']},user='bob',expected=403)
   protocol=api('POST','/api/v1/admin/protocol',{'token':session['token']})
   assert protocol['phase']=='preparing' and not protocol['can_activate'] and protocol['legacy_staking_contract']==escrow.lower()
   def deploy_modular(name,types,values):
    code=json.loads(next((ROOT/'contracts/out').glob('*/'+name+'.json')).read_text())['bytecode']['object']
    return send(admin,None,code+encode(types,values).hex())
   gov=deploy_modular('LaunchpadTimelock',['address[]','address[]'],[[admin],[admin]])['contractAddress']
   modules=deploy_modular('ModuleDeployer',[],[])['contractAddress']
   candidate_receipt=deploy_modular('TestnetProtocolFactory',['address']*7,[token,gov,admin,admin,admin,settler.address,modules])
   factory=candidate_receipt['contractAddress']
   contracts={'factory':factory,'governance':gov}
   for component,getter in [('registry','registry()'),('credit_token','creditToken()'),('usage_escrow','usageEscrow()'),('payment_escrow','paymentEscrow()'),('stake_vault','stakeVault()'),('sale_router','saleRouter()'),('settlement_module','settlementModule()')]:
    contracts[component]='0x'+rpc('eth_call',[{'to':factory,'data':'0x'+selector(getter)},'latest'])[-40:]
   hashes={name:'0x'+keccak(bytes.fromhex(rpc('eth_getCode',[address,'latest'])[2:])).hex() for name,address in contracts.items()}
   candidate={'id':'local-staged','version':'modular-v1','chain_id':31337,'deployment_block':int(candidate_receipt['blockNumber'],16),'prl_token':token,'contracts':contracts,'runtime_hashes':hashes}
   (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[candidate]}))
   rpc('anvil_mine',['0x80'])
   protocol=api('POST','/api/v1/admin/protocol',{'token':session['token']})
   assert protocol['phase']=='staged' and not protocol['can_activate'] and protocol['candidates'][0]['validation']['code_and_bindings']=='passed',protocol
   assert api('GET','/api/v1/status')['chain']['staking_contract']==escrow.lower()
   altered=json.loads(json.dumps(candidate));altered['runtime_hashes']['registry']='0x'+'00'*32
   (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[altered]}))
   protocol=api('POST','/api/v1/admin/protocol',{'token':session['token']})
   assert not protocol['can_activate'] and 'hash mismatch' in protocol['candidates'][0]['validation']['error']
   altered=json.loads(json.dumps(candidate))
   for field_name in ['contracts','runtime_hashes']:altered[field_name]['sale_router'],altered[field_name]['settlement_module']=altered[field_name]['settlement_module'],altered[field_name]['sale_router']
   (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[altered]}))
   protocol=api('POST','/api/v1/admin/protocol',{'token':session['token']})
   assert 'binding mismatch' in protocol['candidates'][0]['validation']['error'] and not protocol['can_activate']
   (tmp/'protocol-groups.json').write_text('{invalid')
   protocol=api('POST','/api/v1/admin/protocol',{'token':session['token']})
   assert protocol['phase']=='blocked' and not protocol['can_activate'] and str(tmp) not in json.dumps(protocol)
   (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[]}))
   print('PASS staged contract groups: real finalized bytecode/bindings/governance audit, tampering rejected, admin isolation, no activation or legacy config mutation.',flush=True)
   admin_sign('prepare',{'project_id':pid},expected=403) # missing VERIFIER_ROLE / capacity
   admin_sign('capacity',{'project_id':pid,'reason':'Manual demo approval','manual_demo':True},provider.key,expected=403)
   admin_sign('retest',{'project_id':pid},expected=409) # no publisher consent yet
   api('POST',path+'/deployment/rpc-credential',{'inference_api_key':'test-inference-secret'},user='bob',expected=404)
   consent=api('POST',path+'/deployment/rpc-credential',{'inference_api_key':'test-inference-secret'})
   assert consent['authorized'] is True and 'test-inference-secret' not in str(consent)
   assert api('GET',path+'/deployment/rpc-credential')['authorized'] is True
   private_file=tmp/'rpc-credentials'/f'{pid}.json';assert private_file.stat().st_mode&0o077==0
   assert next(p for p in api('POST','/api/v1/admin/query',{'token':session['token']})['projects'] if p['_id']==pid)['rpc_retest_available'] is True
   admin_sign('retest',{'project_id':pid},provider.key,expected=403)
   time.sleep(11)
   # Different Google account, correct admin wallet: no publisher login/key required.
   retested,_=admin_sign('retest',{'project_id':pid},user='platform-reviewer')
   assert retested['report']['status']=='passed' and 'test-inference-secret' not in str(retested)
   api('POST',path+'/deployment/rpc-credential',{'revoke':True})
   assert not private_file.exists()
   admin_sign('retest',{'project_id':pid},expected=409)
   assert api('GET',path+'/deployment/rpc-credential')['authorized'] is False
   approved,proof=admin_sign('capacity',{'project_id':pid,'reason':'Manual demo approval','manual_demo':True})
   assert approved['benchmarked'] is False and approved['sales_enabled'] is False
   directory=api('GET','/api/v1/launchpad',user='bob');public=next(p for p in directory['launches'] if p['_id']==pid)
   assert public['capacity_approved'] is True and 'capacity_approval' not in public
   assert credential not in json.dumps(directory) and 'Manual demo approval' not in json.dumps(directory)

   api('POST','/api/v1/admin/capacity',proof,expected=403)
   c=api('POST','/api/v1/admin/challenge',{'intent':'capacity','payload':{'project_id':pid,'reason':'Bound to original payload','manual_demo':True}})
   signed=Account.sign_message(encode_defunct(text=c['message'].replace('Bound to original payload','Tampered approval')),admin_key).signature.hex()
   api('POST','/api/v1/admin/capacity',{'challenge_id':c['challenge_id'],'signature':'0x'+signed.removeprefix('0x')},expected=403)

   saved=issued['launch_id'];time.sleep(11);assert api('GET',path)['project']['launch_id']==saved
   # Contract expiry/refund disables the project without deleting its historic identity.
   rpc('evm_increaseTime',[86401]);rpc('anvil_mine',['0x1'])
   send(provider.address,escrow,'0x'+selector('refundExpired(bytes32)')+registered['onchain_key'][2:])
   refunded=wait(lambda:get_status('refunded'),60);assert refunded['launch_id']==saved
   assert all(p['_id']!=pid for p in api('GET','/api/v1/launchpad',user='bob')['launches'])
   api('POST',path+'/deployment/bundle',{},expected=409);assert hook(login)['reject'] is True
   api('POST',path+'/deployment/rpc-check',{'inference_api_key':'test-inference-secret'},expected=409)
   # A stake refunded before event finality must not poison the global cursor.
   second=api('POST','/api/v1/projects',{'name':'Refund before finality','model_id':'qwen3-8b-awq','request_id':str(uuid.uuid4())})['project']
   second_path='/api/v1/projects/'+second['_id']
   challenge=api('POST',second_path+'/stake/challenge',{'wallet_address':provider.address,'stake_amount':'2000'})
   sig=Account.sign_message(encode_defunct(text=challenge['message']),provider.key).signature.hex()
   api('POST',second_path+'/stake/prepare',{'challenge_id':challenge['challenge_id'],'signature':'0x'+sig.removeprefix('0x')})
   def second_state(status):
    p=api('GET',second_path)['project'];return p if p['status']==status else None
   wait(lambda:second_state('awaiting-stake'),90)
   api('POST',second_path+'/stake/submit',{'tx_hash':tx_hash},expected=400)
   detail=api('GET',second_path+'/stake');send(provider.address,token,detail['approve_data']);send(provider.address,escrow,detail['stake_data'])
   rpc('evm_increaseTime',[86401]);rpc('anvil_mine',['0x1'])
   send(provider.address,escrow,'0x'+selector('refundExpired(bytes32)')+project_key_for_test(second['_id']))
   rpc('anvil_mine',['0x80'])
   early_refund=wait(lambda:second_state('refunded'),90);assert 'launch_id' not in early_refund
   assert not api('GET','/api/v1/status')['chain']['error']
   # Separate launch exercises browser-signed verification and finalized FRPS eligibility.
   send(admin,escrow,'0x'+selector('grantRole(bytes32,address)')+keccak(text='VERIFIER_ROLE').hex()+addr(admin))
   third=api('POST','/api/v1/projects',{'name':'Admin verified launch','model_id':'qwen3-8b-awq','request_id':str(uuid.uuid4())})['project'];third_id=third['_id'];third_path='/api/v1/projects/'+third_id
   c=api('POST',third_path+'/stake/challenge',{'wallet_address':provider.address,'stake_amount':'2000'})
   sig=Account.sign_message(encode_defunct(text=c['message']),provider.key).signature.hex()
   api('POST',third_path+'/stake/prepare',{'challenge_id':c['challenge_id'],'signature':'0x'+sig.removeprefix('0x')})
   wait(lambda:api('GET',third_path)['project']['status']=='awaiting-stake',90)
   detail=api('GET',third_path+'/stake');send(provider.address,token,detail['approve_data']);send(provider.address,escrow,detail['stake_data']);rpc('anvil_mine',['0x80'])
   wait(lambda:api('GET',third_path)['project']['status']=='awaiting-rpc',90)
   third=api('GET',third_path)['project'];bundle=api('POST',third_path+'/deployment/bundle',{})
   with tarfile.open(fileobj=io.BytesIO(base64.b64decode(bundle['bundle_base64'])),mode='r:gz') as archive:
    secret=archive.extractfile('provider-'+third['launch_id']+'/private/token').read().decode()
   third_login={'user':third['provider_id'],'metas':{'provider_token':secret}}
   request=urllib.request.Request(api_url+'/internal/frp/handler?op=NewProxy',data=json.dumps({'content':{'user':third_login,'proxy_type':'http','proxy_name':third['provider_id']+'.inference','custom_domains':['p-'+hashlib.sha256(third['provider_id'].encode()).hexdigest()[:40]+'.launchpad.internal']}}).encode(),headers={'Content-Type':'application/json'})
   assert json.load(urllib.request.urlopen(request))['reject'] is False
   admin_sign('capacity',{'project_id':third_id,'reason':'No passing RPC yet','manual_demo':True},expected=403)
   api('POST',third_path+'/deployment/rpc-check',{'inference_api_key':'test-inference-secret'})
   admin_sign('capacity',{'project_id':third_id,'reason':'Manual integration demo approval','manual_demo':True})
   prepared,_=admin_sign('prepare',{'project_id':third_id})
   tx=prepared['transaction'];assert tx['from'].lower()==admin.lower() and tx['to'].lower()==escrow.lower()
   receipt=send(admin,escrow,tx['data'])
   admin_sign('submit',{'project_id':third_id,'tx_hash':receipt['transactionHash']})
   assert api('GET',third_path)['project']['status']=='verification-pending'
   rpc('anvil_mine',['0x80'])
   wait(lambda:api('GET',third_path)['project']['status']=='accepted',90)
   trial_path='/api/v1/launchpad/'+third_id+'/trial'
   assert not next(p for p in api('GET','/api/v1/launchpad')['launches'] if p['_id']==third_id)['trial_available']
   api('POST',third_path+'/deployment/rpc-credential',{'inference_api_key':'test-inference-secret'})
   api('GET',trial_path,user='buyer',expected=409) # recheck custody does not imply public trial consent
   api('POST',third_path+'/deployment/rpc-credential',{'trial_enabled':True},user='buyer',expected=404)
   assert api('POST',third_path+'/deployment/rpc-credential',{'trial_enabled':True})['trial_enabled'] is True
   info=api('GET',trial_path,user='buyer');assert info['quota']['day_remaining']==100
   trial=lambda prompt:{'prompt':prompt,'request_id':str(uuid.uuid4())}
   api('POST',trial_path,{**trial('hello'),'max_tokens':100000},user='buyer',expected=400)
   api('POST',trial_path,trial('x'*501),user='buyer',expected=400)
   api('POST','/api/v1/launchpad/'+pid+'/trial',trial('hello'),user='buyer',expected=409) # refunded launch
   first=trial('A short greeting please')
   answer=api('POST',trial_path,first,user='buyer');assert answer['text']=='hello' and answer['quota']['day_remaining']==99
   assert 'test-inference-secret' not in str(answer)
   api('POST',trial_path,first,user='buyer',expected=409) # no duplicate generation or extra quota
   assert api('GET',trial_path,user='buyer')['quota']['day_remaining']==99
   api('POST',trial_path,trial('Another greeting'),user='buyer')
   limited=api('POST',trial_path,trial('Third greeting'),user='buyer',expected=429);assert limited['retry_after_seconds']>0
   assert api('GET',trial_path,user='buyer')['quota']['day_remaining']==98
   # A separate authenticated buyer has an independent account allowance.
   assert api('POST',trial_path,trial('Independent buyer'),user='second-buyer')['quota']['day_remaining']==99
   slow=[]
   thread=threading.Thread(target=lambda:slow.append(api('POST',trial_path,trial('slow-trial'),user='concurrent-one')));thread.start();assert trial_started.wait(5)
   api('POST',trial_path,trial('Another simultaneous trial'),user='concurrent-two',expected=429)
   assert api('GET',trial_path,user='concurrent-two')['quota']['day_remaining']==100
   thread.join(timeout=10);assert slow[0]['text']=='hello'
   api('POST',third_path+'/deployment/rpc-credential',{'trial_enabled':False})
   api('POST',trial_path,trial('After revocation'),user='second-buyer',expected=409)
   assert not next(p for p in api('GET','/api/v1/launchpad')['launches'] if p['_id']==third_id)['trial_available']
   rpc('evm_increaseTime',[86401]);rpc('anvil_mine',['0x1'])
   assert hook(third_login)['reject'] is False
   assert api('GET',third_path)['project']['status']=='accepted'
   assert api('GET','/api/v1/status')['sales_enabled'] is False
   # Paid end-to-end flow: real chain escrow, wallet ownership, inference, batch burn, withdrawal.
   credit_path='/api/v1/credits/'+third_id
   api('POST',credit_path+'/purchase',{'quantity':1000},user='paid-buyer',expected=409)
   buyer=Account.create();rpc('anvil_setBalance',[buyer.address,hex(10**18)]);rpc('anvil_impersonateAccount',[buyer.address])
   ch=api('POST','/api/v1/credits/wallet/challenge',{'wallet_address':buyer.address},user='paid-buyer')
   wrong=Account.sign_message(encode_defunct(text=ch['message']),provider.key).signature.hex()
   api('POST','/api/v1/credits/wallet/bind',{'challenge_id':ch['challenge_id'],'signature':'0x'+wrong.removeprefix('0x')},user='paid-buyer',expected=403)
   sig=Account.sign_message(encode_defunct(text=ch['message']),buyer.key).signature.hex();proof={'challenge_id':ch['challenge_id'],'signature':'0x'+sig.removeprefix('0x')}
   api('POST','/api/v1/credits/wallet/bind',proof,user='paid-buyer')
   api('POST','/api/v1/credits/wallet/bind',proof,user='paid-buyer',expected=403)
   api('POST',credit_path+'/purchase',{'quantity':1000},user='paid-buyer',expected=409) # no paid consent / offering
   api('POST',third_path+'/deployment/rpc-credential',{'paid_enabled':True})
   key3=project_key_for_test(third_id)
   send(admin,credits,'0x'+selector('createOffering(bytes32,uint256,uint256,bytes32)')+key3+word(1000000)+word(10**9)+keccak(text='qwen3-8b-awq').hex());rpc('anvil_mine',['0x80'])
   directory=api('GET','/api/v1/launchpad');offer=next(p for p in directory['launches'] if p['_id']==third_id)
   assert offer['sales_enabled'] and offer['credit_offer']['price_wei_per_credit']=='1000000000'
   api('POST',credit_path+'/purchase',{'quantity':1000,'price':0},user='paid-buyer',expected=400)
   prepared=api('POST',credit_path+'/purchase',{'quantity':1000},user='paid-buyer')['transaction']
   assert prepared['from']==buyer.address.lower() and int(prepared['value'],16)==1000*10**9 and prepared['to']==credits.lower()
   purchase_receipt=send(buyer.address,credits,prepared['data'],int(prepared['value'],16))
   pending=api('GET',credit_path,user='paid-buyer');assert pending['available']=='0' and pending['incoming_pending']=='1000'
   history=pending['transaction_history'];assert history['available']
   tracked=next(t for t in history['transactions'] if t['tx_hash']==purchase_receipt['transactionHash']);assert tracked['status']=='awaiting-finality' and tracked['quantity']=='1000' and not tracked['finalized']
   tx_path=credit_path+'/transactions'
   api('POST',tx_path,{'tx_hash':purchase_receipt['transactionHash']},user='alice',expected=409)
   api('POST',tx_path,{'tx_hash':tx_hash},user='paid-buyer',expected=400)
   api('POST',tx_path,{'tx_hash':purchase_receipt['transactionHash']},user='paid-buyer')
   api('POST',tx_path,{'tx_hash':purchase_receipt['transactionHash']},user='paid-buyer')
   unknown='0x'+'1'*64;api('POST',tx_path,{'tx_hash':unknown},user='paid-buyer')
   pending=api('GET',credit_path,user='paid-buyer');assert pending['incoming_pending']=='1000'
   assert next(t for t in pending['transaction_history']['transactions'] if t['tx_hash']==unknown)['status']=='not-seen'
   rpc('evm_setAutomine',[False])
   failed_hash=rpc('eth_sendTransaction',[{'from':buyer.address,'to':credits,'data':prepared['data'],'value':'0x0','gas':'0x800000'}])
   api('POST',tx_path,{'tx_hash':failed_hash},user='paid-buyer')
   history=api('GET',credit_path,user='paid-buyer')['transaction_history'];assert next(t for t in history['transactions'] if t['tx_hash']==failed_hash)['status']=='submitted'
   rpc('evm_setAutomine',[True]);rpc('anvil_mine',['0x1'])
   failed_receipt=wait(lambda:rpc('eth_getTransactionReceipt',[failed_hash]));assert failed_receipt['status']=='0x0'
   api('POST',tx_path,{'tx_hash':failed_hash},user='paid-buyer')
   history=api('GET',credit_path,user='paid-buyer')['transaction_history'];assert next(t for t in history['transactions'] if t['tx_hash']==failed_hash)['status']=='failed'

   rpc('anvil_mine',['0x80'])
   finalized_position=api('GET',credit_path,user='paid-buyer');assert finalized_position['available']=='1000'
   assert next(t for t in finalized_position['transaction_history']['transactions'] if t['tx_hash']==purchase_receipt['transactionHash'])['status']=='finalized'
   request={'request_id':str(uuid.uuid4()),'prompt':'Paid greeting','max_output_tokens':128}
   result=api('POST',credit_path+'/infer',request,user='paid-buyer');assert result['result']['text']=='hello' and result['consumed']==3
   replay=api('POST',credit_path+'/infer',request,user='paid-buyer');assert replay['replayed'] and replay['result']['text']=='hello'
   assert api('GET',credit_path,user='paid-buyer')['available']=='997'
   api('GET',credit_path+'/request/'+result['request_id'],user='alice',expected=409) # no bound wallet
   recovered=api('GET',credit_path+'/request/'+result['request_id'],user='paid-buyer');assert recovered['result']['text']=='hello'
   def finalized_burn():
    time.sleep(1);rpc('anvil_mine',['0x80'])
    b=api('GET',credit_path,user='paid-buyer');return b if b['finalized']['burned']=='3' and b['requests'][0]['status']=='settled' else None
   settled=wait(finalized_burn,150);assert settled['available']=='997' and settled['consumed_pending']=='0'
   collateral_path=third_path+'/collateral'
   api('GET',collateral_path,user='bob',expected=404)
   collateral=api('GET',collateral_path)
   if progressive:
    assert collateral['progressive'] and collateral['finalized']['free']=='6000000000000000' and collateral['finalized']['locked']=='1999994000000000000000'
    api('POST',collateral_path+'/withdraw',{'amount_raw':'6000000000000001'},expected=409)
    api('POST',collateral_path+'/withdraw',{'amount_raw':'1'},user='bob',expected=404)
    prepared_prl=api('POST',collateral_path+'/withdraw',{'amount_raw':'3000000000000000'})['transaction']
    assert prepared_prl['from']==provider.address.lower() and prepared_prl['to']==escrow.lower()
    # Wrong wallet fails at the contract even if it has the prepared calldata.
    bad=rpc('eth_sendTransaction',[{'from':buyer.address,'to':escrow,'data':prepared_prl['data'],'gas':'0x800000'}]);assert wait(lambda:rpc('eth_getTransactionReceipt',[bad]))['status']=='0x0'
    withdrawal=send(provider.address,escrow,prepared_prl['data'])
    api('POST',collateral_path+'/submit',{'tx_hash':withdrawal['transactionHash']})
    pending_prl=api('GET',collateral_path);assert pending_prl['withdrawal_pending_raw']=='3000000000000000' and pending_prl['finalized']['returned']=='0'
    rpc('anvil_mine',['0x80'])
    freed=api('GET',collateral_path);assert freed['finalized']['free']=='3000000000000000' and freed['finalized']['returned']=='3000000000000000' and freed['withdrawals'][0]['status']=='finalized'
    assert api('GET',third_path)['project']['status']=='accepted'
    print('PASS progressive collateral: finalized provider burn unlocks proportional PRL; provider-only partial withdrawal, pending/finalized returned balances and credit/RPC eligibility preserved.',flush=True)
   else:
    assert not collateral['progressive']
    api('POST',collateral_path+'/withdraw',{'amount_raw':'1'},expected=409)

   tx=api('POST',credit_path+'/withdraw',{'quantity':997},user='paid-buyer')['transaction'];send(buyer.address,credits,tx['data']);rpc('anvil_mine',['0x80'])
   api('POST',credit_path+'/infer',{**request,'request_id':str(uuid.uuid4())},user='paid-buyer',expected=409)
   def approved_withdrawal():
    time.sleep(1);rpc('anvil_mine',['0x80']);return api('GET',credit_path,user='paid-buyer')['finalized']['withdrawal_approved']
   wait(approved_withdrawal,150)
   tx=api('POST',credit_path+'/complete-withdrawal',{},user='paid-buyer')['transaction'];send(buyer.address,credits,tx['data']);rpc('anvil_mine',['0x80'])
   withdrawn=api('GET',credit_path,user='paid-buyer');assert withdrawn['available']=='0' and withdrawn['finalized']['withdrawn']=='997'
   tx=api('POST',credit_path+'/deposit',{'quantity':500},user='paid-buyer')['transaction'];send(buyer.address,credits,tx['data']);rpc('anvil_mine',['0x80'])
   assert api('GET',credit_path,user='paid-buyer')['available']=='500'
   assert int(rpc('eth_call',[{'to':credits,'data':'0x'+selector('revenue(address)')+addr(provider.address)},'latest']),16)==3*10**9
   send(provider.address,credits,'0x'+selector('claimRevenue()'))
   print('PASS paid credits: wallet signatures, immutable purchase terms, finality balance fence, real inference, idempotent recovery, persisted batch burn, revenue, withdrawal reconciliation and redeposit; existing PRL unchanged.')
   if not progressive:
    # Return the 497 wallet tokens so the fixed-source migration has no external-token liability.
    tx=api('POST',credit_path+'/deposit',{'quantity':497},user='paid-buyer')['transaction'];send(buyer.address,credits,tx['data']);rpc('anvil_mine',['0x80'])
    assert api('GET',credit_path,user='paid-buyer')['available']=='997'
    gate_code=json.loads((ROOT/'contracts/out/LegacyMigrationGate.sol/LegacyMigrationGate.json').read_text())['bytecode']['object']
    gate=send(admin,None,gate_code+encode(['address','address','address','bytes32'],[factory,escrow,credits,bytes.fromhex(project_key_for_test(third_id))]).hex())['contractAddress']
    candidate['id']='migration-runtime';candidate['migration_gate']=gate;candidate['migration_runtime_hash']='0x'+keccak(bytes.fromhex(rpc('eth_getCode',[gate,'latest'])[2:])).hex();candidate['monitor_enabled']=False
    (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[candidate]}));rpc('anvil_mine',['0x80'])
    gov_plan,_=admin_sign('migration-governance',{'project_id':third_id,'group_id':'migration-runtime','step':'schedule'})
    send(admin,gov_plan['transaction']['to'],gov_plan['transaction']['data'])
    admin_sign('migration-governance',{'project_id':third_id,'group_id':'migration-runtime','step':'execute'},expected=409)
    rpc('evm_increaseTime',[172801]);rpc('evm_mine',[])
    gov_ready,_=admin_sign('migration-governance',{'project_id':third_id,'group_id':'migration-runtime','step':'execute'})
    send(admin,gov_ready['transaction']['to'],gov_ready['transaction']['data'])
    for contract,role_name in [(escrow,'SETTLER_ROLE'),(credits,'SETTLER_ROLE'),(credits,'OFFER_MANAGER_ROLE')]:send(admin,contract,'0x'+selector('grantRole(bytes32,address)')+keccak(text=role_name).hex()+addr(gate))
    new_vault=contracts['stake_vault'];send(provider.address,token,'0x'+selector('approve(address,uint256)')+addr(new_vault)+word(2000*10**18));send(provider.address,gate,'0x'+selector('fund()'),997*10**9)
    candidate['monitor_enabled']=True
    (tmp/'protocol-groups.json').write_text(json.dumps({'schema_version':1,'candidates':[candidate]}));rpc('anvil_mine',['0x80'])
    frozen,_=admin_sign('migration-freeze',{'project_id':third_id,'group_id':'migration-runtime'});assert frozen['phase']=='frozen'
    api('POST',credit_path+'/infer',{**request,'request_id':str(uuid.uuid4())},user='paid-buyer',expected=409)
    plan_result,_=admin_sign('migration-plan',{'project_id':third_id,'group_id':'migration-runtime','holders':buyer.address})
    consent=api('GET','/api/v1/modular/'+third_id+'/consent-plan',user='paid-buyer')
    assert consent['remaining_credits']=='997' and consent['historical_consumed']=='3' and not consent['signed']
    api('GET','/api/v1/modular/'+third_id+'/consent-plan',user='bob',expected=409)
    digest=rpc('eth_call',[{'to':gate,'data':'0x'+selector('consentDigest(bytes32,uint64)')+encode(['bytes32','uint64'],[bytes.fromhex(consent['plan_hash'][2:]),consent['chain_expiry']]).hex()},'latest'])
    wrong_signature=Account._sign_hash(bytes.fromhex(digest[2:]),provider.key).signature.hex()
    api('POST','/api/v1/modular/'+third_id+'/consent',{'plan_hash':consent['plan_hash'],'signature':'0x'+wrong_signature.removeprefix('0x')},user='paid-buyer',expected=403)
    signature=Account._sign_hash(bytes.fromhex(digest[2:]),buyer.key).signature.hex()
    api('POST','/api/v1/modular/'+third_id+'/consent',{'plan_hash':consent['plan_hash'],'signature':'0x'+signature.removeprefix('0x')},user='paid-buyer')
    assert api('GET','/api/v1/modular/'+third_id+'/consent-plan',user='paid-buyer')['signed'] is True
    prepared_migration,_=admin_sign('migration-prepare',{'project_id':third_id,'group_id':'migration-runtime'})
    assert prepared_migration['simulation']=='passed' and prepared_migration['transaction']['to']==gate.lower()
    migration_receipt=send(admin,gate,prepared_migration['transaction']['data']);rpc('anvil_mine',['0x80'])
    def activate_modular():
     value,_=admin_sign('migration-activate',{'project_id':third_id,'group_id':'migration-runtime','tx_hash':migration_receipt['transactionHash']},expected=200)
     return value
    # Allow group log indexing to catch up; only the specific monitor-behind error is retried.
    # Poll read-only group monitor progress before using the one-shot admin authorization.
    def group_indexed():
     report=api('POST','/api/v1/admin/protocol',{'token':session['token']})
     row=report['candidates'][0].get('monitor') or {}
     if report['candidates'][0].get('monitor_error'):print('Group monitor diagnostic:',report['candidates'][0]['monitor_error'],flush=True)
     return row.get('block',0)>=int(migration_receipt['blockNumber'],16)
    wait(group_indexed,150)
    activated=activate_modular();assert activated['phase']=='active'
    assert api('GET','/api/v1/status')['chain']['staking_contract']==escrow.lower()
    mpath='/api/v1/modular/'+third_id
    state=api('GET',mpath,user='paid-buyer');assert state['protocol']=='modular' and state['available']=='997' and state['finalized']['burned']=='3'
    api('POST',credit_path+'/purchase',{'quantity':1},user='paid-buyer',expected=409)
    purchase=api('POST',mpath+'/purchase',{'quantity':100},user='paid-buyer')['transaction'];purchase_tx=send(buyer.address,purchase['to'],purchase['data'],int(purchase['value'],16))
    api('POST',mpath+'/transactions',{'kind':'purchase','quantity':100,'tx_hash':purchase_tx['transactionHash']},user='paid-buyer');rpc('anvil_mine',['0x80'])
    assert api('GET',mpath,user='paid-buyer')['available']=='1097'
    reservation=api('POST',mpath+'/reserve',{'maximum':128},user='paid-buyer');reserve_tx=send(buyer.address,reservation['transaction']['to'],reservation['transaction']['data'])
    api('POST',mpath+'/transactions',{'kind':'reserve','reservation_id':reservation['reservation_id'],'tx_hash':reserve_tx['transactionHash']},user='paid-buyer');rpc('anvil_mine',['0x80'])
    output=api('POST',mpath+'/infer',{'reservation_id':reservation['reservation_id'],'prompt':'Return the integration answer.'},user='paid-buyer');assert output['result']['usage']['completion_tokens']==3
    repeated=api('POST',mpath+'/infer',{'reservation_id':reservation['reservation_id'],'prompt':'Do not run twice.'},user='paid-buyer');assert repeated['replayed'] is True
    def modular_burned():
     rpc('anvil_mine',['0x80']);time.sleep(1);value=api('GET',mpath,user='paid-buyer');return value if value['finalized']['burned']=='6' else False
    modular_state=wait(modular_burned,150);assert modular_state['available']=='1094'
    collateral=api('GET',third_path+'/collateral');assert collateral['protocol']=='modular' and collateral['finalized']['free']=='12000000000000000'
    withdraw=api('POST',third_path+'/collateral/withdraw',{'amount_raw':'1000000000000000'})['transaction'];withdrawal_tx=send(provider.address,withdraw['to'],withdraw['data']);api('POST',third_path+'/collateral/submit',{'tx_hash':withdrawal_tx['transactionHash']});rpc('anvil_mine',['0x80'])
    collateral=api('GET',third_path+'/collateral');assert collateral['finalized']['returned']=='1000000000000000'
    print('PASS full V1 migration + real Rust modular purchase/reserve/inference/burn/PRL withdrawal; identity and legacy source preserved.',flush=True)
   rpc('evm_revert',[snapshot])
   wait(lambda:api('GET','/api/v1/status')['chain']['halted'],60)
   assert api('GET','/api/v1/status')['sales_enabled'] is False
   print('PASS: Rust + MongoDB + Anvil: wrong signature/replay/owner isolation, registration, approve/stake, no early ID, finalized issuance, block-time deadline, stable ID, refund, owner-only RPC checks, saved failures/success, key privacy and expiry; admin wallet signatures, tamper/replay rejection, account-bound review, manual capacity approval, verifier role, finalized acceptance and post-deadline tunnel eligibility; private credential consent/revocation and independent wallet-signed admin RPC retests; real launch directory excludes drafts, private approval details, credentials and inactive projects; real prompt trial consent, quota, input limits, replay prevention and independent account limits.')
  except Exception:
   log.flush();log.seek(0);print(log.read()[-5000:]);raise
  finally:
   for p in reversed(children):
    if p.poll() is None:p.terminate()
   for p in reversed(children):
    try:p.wait(timeout=15)
    except subprocess.TimeoutExpired:p.kill()
   log.close()
if __name__=='__main__':run()
