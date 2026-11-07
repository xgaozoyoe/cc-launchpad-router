import { findModel } from './models.ts';
export interface Project {
  id:string; name:string; modelId:string; stake:number; price:number; total:number; window:string;
  status:'awaiting-stake'|'awaiting-rpc'|'live';
  providerId?:string; launchId?:string; stakedAt?:number; deadline?:number; verifiedAt?:number;
}
export type ProjectAction =
 | {type:'project-create';id:string;name:string;modelId:string;stake:number;price:number;total:number;window:string}
 | {type:'project-stake';id:string;projectId:string;txHash:string;blockTimestamp:number;now:number;launchId:string}
 | {type:'project-verify';id:string;projectId:string;now:number};
export function projectStatus(p:Project,now:number){return p.status==='awaiting-rpc'&&now>=(p.deadline??0)?'expired':p.status;}
export function validProject(p:unknown):p is Project {
 if(!p||typeof p!=='object')return false;
 const v=p as Project;
 if(typeof v.id!=='string'||typeof v.name!=='string'||!findModel(v.modelId)||!['awaiting-stake','awaiting-rpc','live'].includes(v.status))return false;
 if(![v.stake,v.price,v.total].every(n=>Number.isFinite(n)&&n>0)||typeof v.window!=='string')return false;
 if(v.status==='awaiting-stake')return !v.launchId&&!v.providerId;
 return /^0x[0-9a-f]{64}$/.test(v.providerId??'')&&typeof v.launchId==='string'&&Number.isSafeInteger(v.stakedAt)&&v.deadline===v.stakedAt!+86400&&(v.status!=='live'||(Number.isSafeInteger(v.verifiedAt)&&v.verifiedAt!>=v.stakedAt!&&v.verifiedAt!<v.deadline));
}
