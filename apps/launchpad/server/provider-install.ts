import {createHmac,timingSafeEqual} from 'node:crypto';
import {centralProxy} from './central.ts';
import type {CentralEnvironment} from './central.ts';
import {handleAuth,validOrigin} from './auth.ts';
const reply=(body:unknown,status=200)=>Response.json(body,{status,headers:{'Cache-Control':'no-store'}});
type Grant={user:string;project:string;launch:string;expires:number};
export async function providerInstall(request:Request,env:CentralEnvironment,fetcher:typeof fetch=fetch,sessionReader=handleAuth):Promise<Response>{
 const origin=validOrigin(env.APP_ORIGIN),key=env.CENTRAL_HMAC_KEY;
 if(!origin||!key)return reply({error:'Installer service unavailable'},503);
 const url=new URL(request.url);if(request.method!=='POST'||url.search)return reply({error:'Method not allowed'},405);
 const sign=(s:string)=>createHmac('sha256',key).update('provider-install-v1:'+s).digest('hex');
 const proxy=(project:string,action:string,reader:typeof handleAuth,body?:string)=>centralProxy(new Request(origin+'/api/central/projects/'+project+action,{method:body===undefined?'GET':'POST',headers:{origin,cookie:request.headers.get('cookie')??'','Content-Type':'application/json'},body}),env,fetcher,reader);
 if(url.pathname==='/api/provider-install/command'){
  if(request.headers.get('origin')!==origin)return reply({error:'Invalid origin'},403);
  const session=await sessionReader(new Request(origin+'/api/auth/session',{headers:{cookie:request.headers.get('cookie')??''}}),env);const identity=await session.json() as {user?:{id?:string}};if(!session.ok||!identity.user?.id)return reply({error:'Sign-in required'},401);
  const input=await request.json() as {project_id?:string};if(!/^[a-f0-9-]{36}$/.test(input.project_id??''))return reply({error:'Invalid project'},400);const id=input.project_id!;
  const detail=await proxy(id,'',sessionReader);if(!detail.ok)return detail;const {project}=await detail.json() as {project:{launch_id?:string;model_id:string}};
  const eligibility=await proxy(id,'/deployment',sessionReader);if(!eligibility.ok)return eligibility;const setup=await eligibility.json() as {bundle_available:boolean;reason?:string};
  if(!setup.bundle_available||!project.launch_id)return reply({error:setup.reason??'Stake must be confirmed first'},409);
  if(project.model_id!=='qwen3-8b-awq')return reply({error:'One-command deployment currently supports Qwen3 8B AWQ only.'},409);
  const grant:Grant={user:identity.user.id,project:id,launch:project.launch_id,expires:Math.floor(Date.now()/1000)+30*86400};const data=Buffer.from(JSON.stringify(grant)).toString('base64url');
  return reply({command:`curl -fsSL ${origin}/install.sh | bash -s -- --launch-id ${grant.launch} --token ${data}.${sign(data)}`,expires_at:grant.expires});
 }
 let grant:Grant;try{const token=request.headers.get('authorization')?.replace(/^Bearer /,'')??'';const parts=token.split('.');if(parts.length!==2||! /^[a-f0-9]{64}$/.test(parts[1])||!timingSafeEqual(Buffer.from(parts[1]),Buffer.from(sign(parts[0]))))throw Error();grant=JSON.parse(Buffer.from(parts[0],'base64url').toString());if(grant.expires<=Date.now()/1000||! /^[a-f0-9-]{36}$/.test(grant.project))throw Error();}catch{return reply({error:'Invalid or expired deployment token'},401);}
 const input=await request.json() as {launch_id?:string;inference_api_key?:string};if(input.launch_id!==grant.launch)return reply({error:'Token does not authorize this launch'},403);
 const reader:typeof handleAuth=async()=>reply({user:{id:grant.user}});
 // Ownership and eligibility are checked again by the existing Rust endpoints.
 if(url.pathname==='/api/provider-install/bundle')return proxy(grant.project,'/deployment/bundle',reader,'{}');
 if(url.pathname==='/api/provider-install/test')return proxy(grant.project,'/deployment/rpc-check',reader,JSON.stringify({inference_api_key:input.inference_api_key}));
 return reply({error:'Not found'},404);
}
