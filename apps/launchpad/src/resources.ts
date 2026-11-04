import { findModel } from './models';
export interface ComputeResource {
  id:string; name:string; modelId:string; endpoint:string; servedModel:string;
  region:string; hardware:string; readiness:'demo-ready';
}
export function endpointError(value:string):string {
  try {
    const url=new URL(value);
    if(url.username||url.password||url.search||url.hash)return 'Use a base URL without credentials, query parameters, or fragments.';
    const local=['localhost','127.0.0.1','[::1]'].includes(url.hostname);
    if(url.protocol!=='https:'&&!(local&&url.protocol==='http:'))return 'Use HTTPS, or HTTP on localhost for local testing.';
    if(!url.pathname.replace(/\/$/,'').endsWith('/v1'))return 'The base URL should end in /v1.';
    return '';
  } catch {return 'Enter a valid API base URL.';}
}
export function validResource(value:unknown):value is ComputeResource {
  if(!value||typeof value!=='object')return false;
  const r=value as ComputeResource;
  return typeof r.id==='string'&&typeof r.name==='string'&&r.name.length>0&&r.name.length<=60&&
    typeof r.modelId==='string'&&!!findModel(r.modelId)&&typeof r.endpoint==='string'&&!endpointError(r.endpoint)&&
    typeof r.servedModel==='string'&&r.servedModel.length>0&&r.servedModel.length<=120&&
    typeof r.region==='string'&&typeof r.hardware==='string'&&r.readiness==='demo-ready';
}
export function loadResources(key:string):ComputeResource[] {
  try {const data:unknown=JSON.parse(localStorage.getItem(key)??'[]');return Array.isArray(data)?data.filter(validResource):[];}catch{return [];}
}
