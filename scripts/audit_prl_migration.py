"""Read-only, pinned-block migration inventory. Never signs or broadcasts transactions."""
from pathlib import Path
import json,subprocess,hashlib,time,os,argparse
os.umask(0o077)
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--project-root",default="/home/xgao/services/cc-launchpad");parser.add_argument("--project-id",default="076d4cfc-3b2c-4b51-999b-32b88b7a41b5");args=parser.parse_args()
b=Path(args.project_root);cfg=json.loads((b/'private/config.json').read_text());credit=json.loads((b/'deploy/credits-sepolia.json').read_text());legacy=json.loads((b/'deploy/sepolia.json').read_text());cast=str(b/'tools/foundry/cast');pid=args.project_id;key='0x'+hashlib.sha256(pid.encode()).hexdigest();zero='0x'+'0'*40
rpc_args=['--rpc-url',cfg['CHAIN_RPC_URL']]
def run(args):
 assert args[0] in ['rpc','keccak'], 'Audit is read-only'
 p=subprocess.run([cast,*args],capture_output=True,text=True,timeout=30)
 if p.returncode:raise RuntimeError('Migration inventory RPC read failed')
 return p.stdout.strip()
def rpc(method,*args):
 assert method in ['eth_getBlockByNumber','eth_getLogs','eth_call','eth_getBalance'], 'Read-only RPC methods only'
 return json.loads(run(['rpc',method,*[json.dumps(v) for v in args],*rpc_args]))
def hash_(s):return run(['keccak',s])
def word(n):return f'{n:064x}'
def addr(a):return a[2:].rjust(64,'0')
def call(to,sig,args,tag):return rpc('eth_call',{'to':to,'data':hash_(sig)[:10]+''.join(args)},tag)
def words(raw):return [int(raw[i:i+64],16) for i in range(2,len(raw),64)]
head=rpc('eth_getBlockByNumber','latest',False);final=rpc('eth_getBlockByNumber','finalized',False);h=int(head['number'],16);f=int(final['number'],16)
topics={n:hash_(s) for n,s in {'purchase':'CreditsPurchased(bytes32,address,uint256,uint256)','deposited':'CreditsDeposited(bytes32,address,uint256)','withdrawn':'CreditsWithdrawn(bytes32,address,uint256,uint256)','usage':'UsageSettled(bytes32,bytes32,bytes32,address,uint256)','single':'TransferSingle(address,address,address,uint256,uint256)','batch':'TransferBatch(address,address,address,uint256[],uint256[])'}.items()};reverse={v:k for k,v in topics.items()};wallets=set();burns=[]
for start in range(credit['deployment_block'],h+1,100):
 for log in rpc('eth_getLogs',{'address':credit['contract'],'fromBlock':hex(start),'toBlock':hex(min(start+99,h)),'topics':[list(topics.values())]}):
  typ=reverse[log['topics'][0]];data=words(log['data']);ts=log['topics']
  if typ in ['purchase','deposited','withdrawn'] and ts[1]==key:wallets.add('0x'+ts[2][-40:])
  if typ=='usage' and ts[3]==key:
   wallets.add('0x'+word(data[0])[-40:]);burns.append({'usage_id':ts[2],'tx_hash':log['transactionHash'],'block':int(log['blockNumber'],16),'quantity':str(data[1]),'finalized':int(log['blockNumber'],16)<=f})
  if typ=='single' and data[0]==int(key,16):wallets.update(['0x'+ts[2][-40:],'0x'+ts[3][-40:]])
  if typ=='batch':
   offset=data[0]//32;length=data[offset];ids=data[offset+1:offset+1+length]
   if int(key,16) in ids:wallets.update(['0x'+ts[2][-40:],'0x'+ts[3][-40:]])
wallets.discard(zero);wallets.discard(credit['contract'].lower());rows=[]
for wallet in sorted(wallets):
 snapshots={}
 for tag,name in [(final['number'],'finalized'),(head['number'],'latest')]:
  a=words(call(credit['contract'],'accountState(bytes32,address)',[key[2:],addr(wallet)],tag));tokens=words(call(credit['contract'],'balanceOf(address,uint256)',[addr(wallet),key[2:]],tag))[0]
  snapshots[name]={'locked':str(a[0]),'deposited':str(a[1]),'burned':str(a[2]),'withdrawn':str(a[3]),'withdraw_request':str(a[4]),'withdraw_nonce':str(a[5]),'withdraw_approved':bool(a[6]),'withdraw_pending':bool(a[7]),'wallet_tokens':str(tokens)}
 rows.append({'wallet':wallet,**snapshots})
o=words(call(credit['contract'],'offerState(bytes32)',[key[2:]],final['number']));provider='0x'+word(o[0])[-40:];stake=words(call(legacy['staking_contract'],'stakeState(bytes32)',[key[2:]],final['number']));locked=sum(int(r['finalized']['locked']) for r in rows);wallet_tokens=sum(int(r['finalized']['wallet_tokens']) for r in rows);burned=sum(int(r['finalized']['burned']) for r in rows);event_burned=sum(int(v['quantity']) for v in burns if v['finalized']);revenue=words(call(credit['contract'],'revenue(address)',[addr(provider)],final['number']))[0]
out={'read_only':True,'project_id':pid,'project_key':key,'chain_id':11155111,'snapshot_at':int(time.time()),'latest_block':h,'latest_block_hash':head['hash'],'finalized_block':f,'finalized_block_hash':final['hash'],'legacy_staking_contract':legacy['staking_contract'],'legacy_credits_contract':credit['contract'],'token':legacy['token_address'],'provider':provider,'stake_status':stake[0],'initial_prl_raw':str(stake[1]),'committed_credit_supply':str(o[1]),'issued_credits':str(o[2]),'price_wei_per_credit':str(o[3]),'old_sales_open':bool(o[4]),'finalized_locked_credits':str(locked),'finalized_wallet_tokens':str(wallet_tokens),'finalized_burned_credits':str(burned),'finalized_usage_event_sum':str(event_burned),'pending_burn_events':[v for v in burns if not v['finalized']],'provider_unclaimed_revenue_wei':str(revenue),'old_credit_contract_eth_wei':str(int(rpc('eth_getBalance',credit['contract'],final['number']),16)),'prefund_required_wei':str((locked+wallet_tokens)*o[3]),'remaining_unissued_credits':str(o[1]-o[2]),'historical_unlock_if_migrated_raw':str(stake[1]*event_burned//o[1]),'accounts':rows}
assert locked+wallet_tokens+burned==o[2], 'Issuance inventory mismatch'
assert burned==event_burned, 'Legacy settlement event mismatch'
(b/'private/migration-inventory.json').write_text(json.dumps(out,indent=2)+'\n');summary={k:v for k,v in out.items() if k!='accounts'};print(json.dumps(summary,indent=2));print('Per-wallet details saved only to the private inventory file.')
