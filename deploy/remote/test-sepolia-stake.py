#!/usr/bin/env python3
"""Operator-run test with the dedicated test wallet; reports contain no signer secrets."""
import argparse,hashlib,hmac,json,os,secrets,subprocess,time,urllib.request,urllib.error,uuid
from pathlib import Path
base=Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser();parser.add_argument('--account',help='Authenticated application account ID for the test project');parser.add_argument('--wait',type=int,default=0);args=parser.parse_args()
state_file=base/'private/sepolia-stake-test.json';wallet=base/'private/sepolia-wallet';address=(wallet/'address.txt').read_text().strip();key=bytes.fromhex((base/'private/backend-hmac.key').read_text().strip())
if state_file.exists():state=json.loads(state_file.read_text())
else:
 if not args.account:raise SystemExit('Pass the verified application account ID')
 state={'account':args.account,'request_id':str(uuid.uuid4()),'wallet':address,'stake_amount':'2000'}
os.umask(0o077)
def save():state_file.write_text(json.dumps(state,indent=2));state_file.chmod(0o600)
save()
def api(method,path,body=None,user=None,expected=200):
 user=user or state['account'];raw=b'' if body is None else json.dumps(body).encode();stamp=str(int(time.time()));nonce=secrets.token_hex(16)
 envelope='\n'.join((method,path,user,stamp,nonce,hashlib.sha256(raw).hexdigest()));headers={'Content-Type':'application/json','X-CC-User':user,'X-CC-Timestamp':stamp,'X-CC-Nonce':nonce,'X-CC-Signature':hmac.new(key,envelope.encode(),hashlib.sha256).hexdigest()}
 req=urllib.request.Request('https://riddle.xyz'+path,data=raw if method=='POST' else None,method=method,headers=headers)
 try:
  with urllib.request.urlopen(req,timeout=45) as response:code,data=response.status,json.load(response)
 except urllib.error.HTTPError as e:code,data=e.code,json.load(e)
 if code!=expected:raise RuntimeError(f'API {path} returned {code}: '+str(data.get('error','unexpected response')))
 return data
cast=str(base/'tools/foundry/cast');signer=['--keystore',str(wallet/'keystores/sepolia-deployer'),'--password-file',str(wallet/'password')];rpc='https://ethereum-sepolia-rpc.publicnode.com'
def send(to,signature,*arguments,gas):
 out=subprocess.run([cast,'send','--json','--rpc-url',rpc,to,signature,*arguments,*signer,'--gas-limit',str(gas)],capture_output=True,text=True)
 if out.returncode:raise RuntimeError('Test transaction command failed; inspect receipts before retrying')
 receipt=json.loads(out.stdout)
 if int(str(receipt['status']),0)!=1:raise RuntimeError('Test transaction reverted')
 return receipt
if 'project_id' not in state:
 p=api('POST','/api/v1/projects',{'name':'Sepolia Stake QA · 2000 tPRL','model_id':'qwen3-8b-awq','request_id':state['request_id']})['project'];state['project_id']=p['_id'];save()
path='/api/v1/projects/'+state['project_id'];p=api('GET',path)['project'];api('GET',path,user='stake-isolation-negative-test',expected=404)
if p['status']=='draft':
 c=api('POST',path+'/stake/challenge',{'wallet_address':address,'stake_amount':state['stake_amount']})
 signed=subprocess.run([cast,'wallet','sign',*signer,c['message']],capture_output=True,text=True,check=True).stdout.strip()
 body={'challenge_id':c['challenge_id'],'signature':signed}
 api('POST',path+'/stake/prepare',body);api('POST',path+'/stake/prepare',body,expected=400)
 print('Wallet ownership verified; challenge replay and cross-account access rejected.',flush=True)
for _ in range(90):
 p=api('GET',path)['project']
 if p['status']!='awaiting-registration':break
 if p.get('registration_error'):raise RuntimeError(p['registration_error'])
 time.sleep(2)
else:raise RuntimeError('Registration is still pending; rerun only after checking status')
if p['status']=='awaiting-stake' and 'stake_tx' not in state:
 if state.get('stake_send_started'):raise RuntimeError('A stake send was started previously; inspect wallet nonce/logs before retrying')
 config=json.loads((base/'deploy/sepolia.json').read_text());amount=str(int(state['stake_amount'])*10**18)
 approval=send(config['token_address'],'approve(address,uint256)',config['staking_contract'],amount,gas=1000000)
 state['approval_tx']=approval['transactionHash'];save();print('Test token approval confirmed:',state['approval_tx'],flush=True)
 state['stake_send_started']=True;save()
 receipt=send(config['staking_contract'],'stake(bytes32)',p['onchain_key'],gas=4000000)
 state['stake_tx']=receipt['transactionHash'];state['stake_block']=int(str(receipt['blockNumber']),0);save()
 api('POST',path+'/stake/submit',{'tx_hash':state['stake_tx']})
 print('Sepolia stake confirmed:',state['stake_tx'],'block:',state['stake_block'],flush=True)
if 'stake_tx' not in state:raise RuntimeError('No test stake hash recorded')
end=time.monotonic()+args.wait
while True:
 p=api('GET',path)['project'];status=api('GET','/api/v1/status')
 state['project_status']=p['status'];state['monitor']=status['chain'];save()
 if p.get('launch_id'):
  assert p['provider_id']==state['stake_tx'].lower() and p['rpc_deadline']==p['staked_at']+86400 and p['stake_finalized'] is True
  result={k:state[k] for k in ('project_id','wallet','stake_amount','approval_tx','stake_tx','stake_block')}
  result.update({k:p[k] for k in ('status','provider_id','launch_id','staked_at','rpc_deadline','stake_finalized')});result['chain_id']=11155111;result['sales_enabled']=False
  (base/'deploy/stake-test-result.json').write_text(json.dumps(result,indent=2)+'\n')
  print('PASS: finalized stake issued Launch ID; provider hash and block-time +24h deadline verified.',flush=True);print(json.dumps(result,indent=2),flush=True);break
 if status['chain'].get('halted') or status['chain'].get('error'):raise RuntimeError('Monitor needs attention: '+str(status['chain']))
 print('Waiting for finality; project:',p['status'],'indexed finalized block:',status['chain'].get('finalized_block'),'stake block:',state['stake_block'],flush=True)
 if time.monotonic()>=end:break
 time.sleep(30)
