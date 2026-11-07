import type {ServerProject} from './server-projects.ts';
export type LaunchGroup='all'|'draft'|'staking'|'rpc'|'live'|'inactive'|'other';
export const launchGroups:ReadonlyArray<{id:LaunchGroup;label:string}>=[{id:'all',label:'All projects'},{id:'draft',label:'Drafts'},{id:'staking',label:'Pending stake'},{id:'rpc',label:'Awaiting RPC'},{id:'live',label:'Selling credits'},{id:'inactive',label:'Inactive'}];
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
