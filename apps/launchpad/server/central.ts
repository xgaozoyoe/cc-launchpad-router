import { createHmac, createHash, randomBytes } from 'node:crypto';
import { handleAuth, validOrigin } from './auth.ts';
import type { AuthEnvironment } from './auth.ts';
export interface CentralEnvironment extends AuthEnvironment { CENTRAL_API_ORIGIN?:string; CENTRAL_HMAC_KEY?:string }
const json=(body:unknown,status=200)=>Response.json(body,{status,headers:{'Cache-Control':'no-store'}});
export async function centralProxy(request:Request,env:CentralEnvironment,fetcher:typeof fetch=fetch,sessionReader=handleAuth):Promise<Response>{
 const url=new URL(request.url),origin=validOrigin(env.APP_ORIGIN);
 if(!origin)return json({error:'Application origin is not configured.'},503);
 const match=url.pathname.match(/^\/api\/central\/(admin\/(?:challenge|authenticate|query|capacity|prepare|submit|retest)|status|models|launchpad|projects(?:\/[0-9a-f-]{36}(?:\/stake(?:\/(?:challenge|prepare|submit))?|\/deployment(?:\/(?:bundle|rpc-check|rpc-credential))?)?)?)$/);
 if(!match||url.search)return json({error:'Endpoint not found.'},404);
 if(!['GET','POST'].includes(request.method)||(request.method==='POST'&&match[1]!=='projects'&&!/^admin\/(challenge|authenticate|query|capacity|prepare|submit|retest)$/.test(match[1])&&!/^projects\/[0-9a-f-]{36}\/(?:stake\/(challenge|prepare|submit)|deployment\/(?:bundle|rpc-check|rpc-credential))$/.test(match[1])))return json({error:'Method not allowed.'},405);
 if(request.method==='GET'&&/(?:\/stake\/(challenge|prepare|submit)|\/deployment\/bundle)$/.test(match[1]))return json({error:'Method not allowed.'},405);
 if(request.method==='POST'&&request.headers.get('origin')!==origin)return json({error:'Invalid request origin.'},403);
 const sessionResponse=await sessionReader(new Request(origin+'/api/auth/session',{headers:{cookie:request.headers.get('cookie')??''}}),env);
 if(!sessionResponse.ok)return json({error:'Sign-in required.'},401);
 const session=await sessionResponse.json() as {user?:{id?:unknown}};
 const user=session?.user?.id;
 if(typeof user!=='string'||!/^[a-zA-Z0-9_.:-]{1,128}$/.test(user))return json({error:'Sign-in required.'},401);
 let base:URL;
 try{base=new URL(env.CENTRAL_API_ORIGIN??'');if(base.username||base.password||base.pathname!=='/'||base.search||base.hash||!(base.protocol==='https:'||(base.protocol==='http:'&&['central','localhost','127.0.0.1'].includes(base.hostname))))throw new Error();}catch{return json({error:'Central backend is not configured.'},503);}
 if(!/^[a-fA-F0-9]{64}$/.test(env.CENTRAL_HMAC_KEY??''))return json({error:'Central signing key is not configured.'},503);
 const body=await request.text();if(Buffer.byteLength(body)>65536)return json({error:'Request too large.'},413);
 const path='/api/v1/'+match[1],stamp=Math.floor(Date.now()/1000).toString(),nonce=randomBytes(16).toString('hex');
 const envelope=[request.method,path,user,stamp,nonce,createHash('sha256').update(body).digest('hex')].join('\n');
 const signature=createHmac('sha256',Buffer.from(env.CENTRAL_HMAC_KEY!,'hex')).update(envelope).digest('hex');
 try{
  const upstream=await fetcher(new URL(path,base),{method:request.method,headers:{'Content-Type':'application/json','X-CC-User':user,'X-CC-Timestamp':stamp,'X-CC-Nonce':nonce,'X-CC-Signature':signature},body:request.method==='POST'?body:undefined,redirect:'error',signal:AbortSignal.timeout(60000)});
  if(match[1].endsWith('/deployment/bundle')&&upstream.ok){
   const payload=await upstream.json() as {filename?:string;bundle_base64?:string};
   if(!/^provider-launch-[a-f0-9-]{36}-linux-amd64\.tar\.gz$/.test(payload.filename??'')||typeof payload.bundle_base64!=='string'||payload.bundle_base64.length>40000000)return json({error:'Invalid bundle response.'},502);
   const response=new Response(Buffer.from(payload.bundle_base64,'base64'),{headers:{'Content-Type':'application/gzip','Content-Disposition':'attachment; filename="'+payload.filename+'"','Cache-Control':'no-store, private','X-Content-Type-Options':'nosniff','Referrer-Policy':'no-referrer'}});
   for(const cookie of sessionResponse.headers.getSetCookie())response.headers.append('Set-Cookie',cookie);
   return response;
  }
  const response=new Response(upstream.body,{status:upstream.status,headers:{'Content-Type':'application/json','Cache-Control':'no-store','X-Content-Type-Options':'nosniff'}});
  for(const cookie of sessionResponse.headers.getSetCookie())response.headers.append('Set-Cookie',cookie);
  return response;
 }catch{return json({error:'Central backend is unavailable. Please retry.'},503);}
}
