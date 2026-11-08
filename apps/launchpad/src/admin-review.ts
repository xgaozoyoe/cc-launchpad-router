import type {ServerProject} from './server-projects.ts';
export function approvalBlocker(project:ServerProject,now:number):string|null{
 if(project.status!=='awaiting-rpc')return 'This project is not waiting for launch approval.';
 if(!project.stake_finalized)return 'Waiting for finalized stake.';
 if(!project.provider_id)return 'Waiting for the finalized provider identity.';
 if(!project.rpc_deadline)return 'Waiting for the on-chain RPC deadline.';
 if(project.rpc_deadline<=now)return 'The RPC acceptance deadline has expired.';
 if(project.progress?.rpc_status!=='passed')return 'RPC verification has not passed.';
 const checked=project.progress.rpc_checked_at??0;
 if(checked>now||checked<=now-3600)return 'RPC verification is stale or invalid. Run Test RPC again.';
 return null;
}
export function reviewCategory(project:ServerProject,now:number):'ready'|'waiting'|'pending'|'accepted'{
 if(project.status==='verification-pending')return 'pending';
 if(project.status==='accepted')return 'accepted';
 return approvalBlocker(project,now)===null?'ready':'waiting';
}
