import type {ServerProject} from './server-projects.ts';
export type LaunchGroup='all'|'draft'|'staking'|'rpc'|'live'|'inactive'|'other';
export const launchGroups:ReadonlyArray<{id:LaunchGroup;label:string}>=[{id:'all',label:'All projects'},{id:'draft',label:'Drafts'},{id:'staking',label:'Pending stake'},{id:'rpc',label:'Setup & acceptance'},{id:'live',label:'Selling credits'},{id:'inactive',label:'Inactive'}];
export function launchGroup(status:string):LaunchGroup {
 if(status==='draft')return 'draft';
 if(['awaiting-registration','awaiting-stake','stake-submitted'].includes(status))return 'staking';
 if(status==='awaiting-rpc')return 'rpc';
 if(status==='live')return 'live';
 if(['expired','refunded','released','chain-halted'].includes(status))return 'inactive';
 return 'other';
}
export function launchLabel(status:string){return ({draft:'Draft','awaiting-registration':'Registration pending','awaiting-stake':'Awaiting stake','stake-submitted':'Waiting for finality','awaiting-rpc':'Awaiting RPC',live:'Selling credits',expired:'Expired',refunded:'Refunded',released:'Released','chain-halted':'Chain review required'} as Record<string,string>)[status]??status;}
export function launchAction(status:string){return ({draft:'Continue staking','awaiting-registration':'View registration','awaiting-stake':'Stake tPRL','stake-submitted':'Track confirmation','awaiting-rpc':'View setup requirements',live:'View launch'} as Record<string,string>)[status]??'View record';}
export function filterLaunches(projects:ServerProject[],group:LaunchGroup,query:string){
 const term=query.trim().toLowerCase();
 return projects.filter(p=>(group==='all'||launchGroup(p.status)===group)&&(!term||[p.name,p.model_id,p._id,p.launch_id,p.provider_id,p.wallet_address].some(value=>value?.toLowerCase().includes(term))));
}
export function stakeUnits(raw?:string){if(!raw)return 'Not committed';try{return (BigInt(raw)/10n**18n).toLocaleString()+' tPRL';}catch{return 'Unavailable';}}

// Current escrow transfers the whole stake on refund/release. It has no
// partial unlock ledger, so never infer a withdrawable balance from usage.
export function stakeBalances(project:ServerProject){
 if(!project.stake_finalized||!project.stake_raw)return null;
 try{
  const total=BigInt(project.stake_raw);
  if(total<0n)return null;
  const returned=['released','refunded'].includes(project.status);
  return {locked:returned?0n:total,free:0n,returned:returned?total:0n};
 }catch{return null;}
}

export type ProgressStep={label:string;state:'complete'|'current'|'upcoming'|'blocked';detail:string};
export function launchProgress(project:ServerProject):ProgressStep[]{
 const inactive=['expired','refunded','released','chain-halted'].includes(project.status);
 const live=project.status==='live';
 const rpc=project.progress?.rpc_status==='passed';
 const completed=[true,project.stake_finalized===true,!!project.progress?.last_proxy_at,rpc,live,live];
 const current=completed.findIndex(done=>!done);
 const details=[
  'Project saved to your account',
  project.stake_finalized?'Finalized stake recorded':'Submit stake and wait for chain finality',
  project.progress?.last_proxy_at?'Proxy registration recorded; not continuous health monitoring':'Start the assigned FRPC bundle on your model host',
  rpc?'Saved API checks passed':project.progress?.rpc_status==='failed'?'Last API test failed; correct the issue and retest':'Run Test RPC through the assigned tunnel',
  live?'Launch accepted':'Platform capacity and on-chain acceptance are not implemented yet',
  live?'Credit sales open':'Sales remain closed until acceptance and sales integration'
 ];
 return ['Project created','Stake finalized','FRPC registered','API checks passed','Capacity & on-chain acceptance','Credit sales'].map((label,i)=>({label,state:completed[i]?'complete':inactive?'blocked':i===current?'current':'upcoming',detail:details[i]}));
}
export function projectBadge(project:ServerProject){
 if(project.status!=='awaiting-rpc')return launchLabel(project.status);
 return project.progress?.rpc_status==='passed'?'Awaiting capacity acceptance':project.progress?.last_proxy_at?'Awaiting API checks':'Awaiting connector';
}
export function projectAction(project:ServerProject){return project.status==='awaiting-rpc'&&project.progress?.rpc_status==='passed'?'View verification results':launchAction(project.status);}
