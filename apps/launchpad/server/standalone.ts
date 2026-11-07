import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { resolve, extname, sep } from 'node:path';
import { handleAuth, validOrigin, configured } from './auth.js';
import { centralProxy } from './central.js';
import type { CentralEnvironment } from './central.js';
const env:CentralEnvironment={...process.env};
if(process.env.CENTRAL_HMAC_KEY_FILE)env.CENTRAL_HMAC_KEY=(await readFile(process.env.CENTRAL_HMAC_KEY_FILE,'utf8')).trim();
const origin=validOrigin(env.APP_ORIGIN);
if(!origin)throw new Error('APP_ORIGIN must be an HTTPS origin or localhost HTTP.');
const local=process.env.LOCAL_TEST_ACCOUNT==='1';
const host=new URL(origin).hostname;
if(local&&(process.env.NODE_ENV==='production'||!['localhost','127.0.0.1'].includes(host)))throw new Error('Local test account is restricted to local development.');
if(!local&&!configured(env))throw new Error('Google login configuration is required.');
const localSession=async()=>Response.json({user:{id:'local-test-account',name:'Local test account',email:'local-test@example.invalid'}});
const root=resolve(process.env.STATIC_DIR??'dist');
const mime:Record<string,string>={'.html':'text/html; charset=utf-8','.js':'text/javascript','.css':'text/css','.svg':'image/svg+xml','.png':'image/png','.pdf':'application/pdf','.pptx':'application/vnd.openxmlformats-officedocument.presentationml.presentation'};
const server=createServer(async(req,res)=>{
 try{
  const url=new URL(req.url??'/',origin);
  const headers=new Headers();for(const [key,value] of Object.entries(req.headers))if(value!==undefined)headers.set(key,Array.isArray(value)?value.join(', '):value);
  const chunks:Buffer[]=[];let size=0;
  for await(const chunk of req){size+=chunk.length;if(size>65536){res.writeHead(413);res.end();return;}chunks.push(Buffer.from(chunk));}
  const request=new Request(url,{method:req.method,headers,body:['GET','HEAD'].includes(req.method??'GET')?undefined:Buffer.concat(chunks)});
  let response:Response;
  // Local bypass never accepts identity supplied by the browser and is blocked in production.
  if(local&&!['127.0.0.1','::1','::ffff:127.0.0.1'].includes(req.socket.remoteAddress??'')){res.writeHead(403);res.end();return;}
  if(url.pathname==='/api/runtime')response=Response.json({integrated:true,localTestAccount:local},{headers:{'Cache-Control':'no-store'}});
  else if(url.pathname==='/healthz'||url.pathname.startsWith('/api/v1/')){
   const base=new URL(env.CENTRAL_API_ORIGIN??'');
   if(!['http:','https:'].includes(base.protocol)||base.username||base.password||base.pathname!=='/'||base.search||base.hash)throw new Error('Invalid backend origin');
   if(base.protocol==='http:'&&!['localhost','127.0.0.1','central'].includes(base.hostname))throw new Error('Backend requires HTTPS');
   const upstreamHeaders=new Headers({'Content-Type':'application/json'});
   for(const name of ['x-cc-user','x-cc-timestamp','x-cc-nonce','x-cc-signature']){const value=request.headers.get(name);if(value)upstreamHeaders.set(name,value);}
   const upstream=await fetch(new URL(url.pathname+url.search,base),{method:req.method,headers:upstreamHeaders,body:['GET','HEAD'].includes(req.method??'GET')?undefined:Buffer.concat(chunks),redirect:'error',signal:AbortSignal.timeout(10000)});
   response=new Response(upstream.body,{status:upstream.status,headers:{'Content-Type':'application/json','Cache-Control':'no-store'}});
  }
  else if(url.pathname.startsWith('/api/auth/')){
   if(local)response=url.pathname==='/api/auth/session'?await localSession():Response.json({configured:true,localTestAccount:true});
   else response=await handleAuth(request,env);
  }else if(url.pathname.startsWith('/api/central/'))response=await centralProxy(request,env,fetch,local?localSession:handleAuth);
  else if(url.pathname.startsWith('/api/'))response=new Response('Not found',{status:404});
  else if(!['GET','HEAD'].includes(req.method??'GET'))response=new Response('Method not allowed',{status:405});
  else{
   let file=resolve(root,'.'+decodeURIComponent(url.pathname));
   if(file!==root&&!file.startsWith(root+sep)){res.writeHead(403);res.end();return;}
   try{if(!(await stat(file)).isFile())file=resolve(root,'index.html');}catch{file=resolve(root,'index.html');}
   response=new Response(await readFile(file),{headers:{'Content-Type':mime[extname(file)]??'application/octet-stream'}});
  }
  res.statusCode=response.status;response.headers.forEach((v,k)=>{if(k!=='set-cookie')res.setHeader(k,v);});const cookies=response.headers.getSetCookie();if(cookies.length)res.setHeader('Set-Cookie',cookies);
  res.end(req.method==='HEAD'?undefined:Buffer.from(await response.arrayBuffer()));
 }catch{res.writeHead(500,{'Content-Type':'application/json','Cache-Control':'no-store'});res.end('{"error":"Application request failed."}');}
});
server.listen(Number(process.env.PORT??4173),process.env.BIND_HOST??'127.0.0.1',()=>console.log(`Launchpad ready at ${origin}${local?' (local test account)':''}`));
for(const signal of ['SIGTERM','SIGINT'] as const)process.on(signal,()=>server.close(()=>process.exit(0)));
