"""Explicit integration test: disposable Anvil + authenticated MongoDB + real Rust API.
Run with an eth-account Python environment after cargo build and forge build.
"""
import base64,hashlib,hmac,json,os,secrets,socket,subprocess,tempfile,time,urllib.request,urllib.error,uuid
from pathlib import Path
from eth_account import Account
from eth_account.messages import encode_defunct
from eth_utils import keccak
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
  def send(sender,to,data):
   tx={'from':sender,'data':data,'gas':'0x400000'}
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
   start([str(FOUNDRY/'anvil'),'--host','127.0.0.1','--port',str(chain_port),'--silent'])
   admin=wait(lambda:rpc('eth_accounts',[]))[0]
   token_bytecode=json.loads((ROOT/'contracts/out/TestPRL.sol/TestPRL.json').read_text())['bytecode']['object']
   stake_bytecode=json.loads((ROOT/'contracts/out/LaunchStake.sol/LaunchStake.json').read_text())['bytecode']['object']
   token_receipt=send(admin,None,token_bytecode+addr(admin)+word(1000000*10**18));token=token_receipt['contractAddress']
   escrow=send(admin,None,stake_bytecode+addr(token)+addr(admin))['contractAddress']
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
   info=api('GET',trial_path,user='buyer');assert info['quota']['day_remaining']==10
   trial=lambda prompt:{'prompt':prompt,'request_id':str(uuid.uuid4())}
   api('POST',trial_path,{**trial('hello'),'max_tokens':100000},user='buyer',expected=400)
   api('POST',trial_path,trial('x'*501),user='buyer',expected=400)
   api('POST','/api/v1/launchpad/'+pid+'/trial',trial('hello'),user='buyer',expected=409) # refunded launch
   first=trial('A short greeting please')
   answer=api('POST',trial_path,first,user='buyer');assert answer['text']=='hello' and answer['quota']['day_remaining']==9
   assert 'test-inference-secret' not in str(answer)
   api('POST',trial_path,first,user='buyer',expected=409) # no duplicate generation or extra quota
   assert api('GET',trial_path,user='buyer')['quota']['day_remaining']==9
   api('POST',trial_path,trial('Another greeting'),user='buyer')
   limited=api('POST',trial_path,trial('Third greeting'),user='buyer',expected=429);assert limited['retry_after_seconds']>0
   assert api('GET',trial_path,user='buyer')['quota']['day_remaining']==8
   # A separate authenticated buyer has an independent account allowance.
   assert api('POST',trial_path,trial('Independent buyer'),user='second-buyer')['quota']['day_remaining']==9
   slow=[]
   thread=threading.Thread(target=lambda:slow.append(api('POST',trial_path,trial('slow-trial'),user='concurrent-one')));thread.start();assert trial_started.wait(5)
   api('POST',trial_path,trial('Another simultaneous trial'),user='concurrent-two',expected=429)
   assert api('GET',trial_path,user='concurrent-two')['quota']['day_remaining']==10
   thread.join(timeout=10);assert slow[0]['text']=='hello'
   api('POST',third_path+'/deployment/rpc-credential',{'trial_enabled':False})
   api('POST',trial_path,trial('After revocation'),user='second-buyer',expected=409)
   assert not next(p for p in api('GET','/api/v1/launchpad')['launches'] if p['_id']==third_id)['trial_available']
   rpc('evm_increaseTime',[86401]);rpc('anvil_mine',['0x1'])
   assert hook(third_login)['reject'] is False
   assert api('GET',third_path)['project']['status']=='accepted'
   assert api('GET','/api/v1/status')['sales_enabled'] is False
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
