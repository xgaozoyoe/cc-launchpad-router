import type { Family, Launch } from './types';
export interface SupportedModel {id:string;name:string;family:Family;precision:string;context:string;description:string}
export const supportedModels:readonly SupportedModel[]=[
  {id:'qwen3-8b-awq',name:'Qwen3 8B AWQ',family:'Qwen',precision:'AWQ 4-bit',context:'8K default context',description:'Concrete Docker demo profile: Qwen/Qwen3-8B-AWQ. Configure a supplier endpoint before issuing demo credits.'},
  {id:'atlas-8b',name:'Atlas 8B Instruct',family:'Llama',precision:'FP16',context:'32K context',description:'General instruction and chat workloads.'},
  {id:'nova-14b',name:'Nova 14B Reasoning',family:'Qwen',precision:'BF16',context:'64K context',description:'Reasoning-oriented inference workloads.'},
  {id:'mistral-7b',name:'Mistral 7B Instruct',family:'Mistral',precision:'FP16',context:'32K context',description:'Compact instruction-following workloads.'}
];
export function findModel(id:string){return supportedModels.find(m=>m.id===id);}
export function legacyModelId(launch:Pick<Launch,'family'>):string{return supportedModels.find(m=>m.family===launch.family&&m.id!=='qwen3-8b-awq')!.id;}
export function offersForModel(launches:readonly Launch[],modelId:string):Launch[]{
  return launches.filter(l=>l.modelId===modelId&&l.sold<l.total).toSorted((a,b)=>a.price-b.price||a.supplier.localeCompare(b.supplier)||a.id.localeCompare(b.id));
}
export function modelName(launch:Launch){return findModel(launch.modelId)?.name??launch.name;}
function alternative(id:string,modelId:string,supplier:string,price:number):Launch {
  const m=findModel(modelId)!;
  return {id,modelId,name:`${supplier} ${m.name} Credits`,supplier,family:m.family,precision:m.precision,context:m.context,price,total:40000000,sold:0,stake:6000,logo:m.name[0],color:m.family==='Qwen'?'blue':m.family==='Mistral'?'purple':'',window:'Nov 2026',own:false};
}
export const alternativeOffers:readonly Launch[]=[
  alternative('atlas-greengrid','atlas-8b','GreenGrid',2.1),
  alternative('nova-northstar','nova-14b','Northstar Compute',4.2),
  alternative('mistral-vertex','mistral-7b','Vertex Cloud',2)
];
