import test from 'node:test';
import assert from 'node:assert/strict';
import {createHmac,createHash} from 'node:crypto';
import {centralProxy} from './central.ts';
const env={APP_ORIGIN:'http://localhost:4174',CENTRAL_API_ORIGIN:'http://127.0.0.1:9001',CENTRAL_HMAC_KEY:'07'.repeat(32)};
const session=async()=>Response.json({user:{id:'verified-google-user'}});
test('proxy signs verified session and ignores browser identity headers',async()=>{
 let calls=0;
 const transport:typeof fetch=async(input,options)=>{
  calls++;assert.equal(String(input),'http://127.0.0.1:9001/api/v1/projects');
  const h=new Headers(options?.headers);assert.equal(h.get('x-cc-user'),'verified-google-user');assert.equal(h.get('cookie'),null);
  const message=['POST','/api/v1/projects','verified-google-user',h.get('x-cc-timestamp'),h.get('x-cc-nonce'),createHash('sha256').update('{}').digest('hex')].join('\n');
  assert.equal(h.get('x-cc-signature'),createHmac('sha256',Buffer.from(env.CENTRAL_HMAC_KEY,'hex')).update(message).digest('hex'));
  return Response.json({project:{_id:'example'}});
 };
 const result=await centralProxy(new Request(env.APP_ORIGIN+'/api/central/projects',{method:'POST',headers:{origin:env.APP_ORIGIN,'x-cc-user':'attacker'},body:'{}'}),env,transport,session);
 assert.equal(result.status,200);assert.equal(calls,1);assert.equal(result.headers.get('cache-control'),'no-store');
});
test('unauthenticated, cross-origin and unknown paths never reach backend',async()=>{
 const transport:typeof fetch=async()=>{throw new Error('must not call backend');};
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/projects'),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/projects',{method:'POST',headers:{origin:'https://attacker.example'},body:'{}'}),env,transport,session)).status,403);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/admin'),env,transport,session)).status,404);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/projects?other=1'),env,transport,session)).status,404);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/account/link-legacy',{method:'POST',headers:{origin:env.APP_ORIGIN},body:'{}'}),env,transport,session)).status,404);
});
test('configuration fails closed and outages are actionable',async()=>{
 const req=()=>new Request(env.APP_ORIGIN+'/api/central/status');
 assert.equal((await centralProxy(req(),{...env,CENTRAL_HMAC_KEY:''},fetch,session)).status,503);
 assert.equal((await centralProxy(req(),{...env,CENTRAL_API_ORIGIN:'http://attacker.example'},fetch,session)).status,503);
 assert.equal((await centralProxy(req(),env,async()=>{throw new Error('private connection detail');},session)).status,503);
});

test('stake routes preserve signing path and cannot bypass server verification',async()=>{
 const id='11111111-1111-4111-8111-111111111111';
 const input=new Request(env.APP_ORIGIN+'/api/central/projects/'+id+'/stake/prepare',{method:'POST',headers:{origin:env.APP_ORIGIN},body:'{}'});
 const response=await centralProxy(input,env,async(url,options)=>{assert.equal(String(url),env.CENTRAL_API_ORIGIN+'/api/v1/projects/'+id+'/stake/prepare');assert.equal(new Headers(options?.headers).get('x-cc-user'),'verified-google-user');return Response.json({error:'Wallet signature does not match'},{status:400});},session);
 assert.equal(response.status,400);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/projects/'+id+'/stake/prepare'),env,fetch,session)).status,405);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/projects/'+id+'/stake/verify-rpc',{method:'POST',headers:{origin:env.APP_ORIGIN},body:'{}'}),env,fetch,session)).status,404);
});

test('bundle download requires session and same-origin POST; streams only a private attachment',async()=>{
 const path='/api/central/projects/11111111-1111-4111-8111-111111111111/deployment/bundle';
 const req=(origin=env.APP_ORIGIN)=>new Request(env.APP_ORIGIN+path,{method:'POST',headers:{origin},body:'{}'});
 const filename='provider-launch-11111111-1111-4111-8111-111111111111-linux-amd64.tar.gz';
 const fetcher:typeof fetch=async()=>Response.json({filename,bundle_base64:Buffer.from('archive-bytes').toString('base64')});
 assert.equal((await centralProxy(req(),env,fetcher,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(req('https://attacker.example'),env,fetcher,session)).status,403);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+path),env,fetcher,session)).status,405);
 const r=await centralProxy(req(),env,fetcher,session);assert.equal(r.headers.get('content-type'),'application/gzip');assert.equal(r.headers.get('cache-control'),'no-store, private');assert.match(r.headers.get('content-disposition')??'',/attachment/);assert.equal(await r.text(),'archive-bytes');
});

test('RPC checks require owner session and same-origin POST; GET reads only saved status',async()=>{
 const path='/api/central/projects/11111111-1111-4111-8111-111111111111/deployment/rpc-check';
 const fetcher:typeof fetch=async(url,options)=>{assert.match(String(url),/deployment\/rpc-check$/);assert.equal(new Headers(options?.headers).get('x-cc-user'),'verified-google-user');return Response.json({report:{status:'passed'},capacity_accepted:false,sales_enabled:false});};
 const req=(origin:string)=>new Request(env.APP_ORIGIN+path,{method:'POST',headers:{origin},body:JSON.stringify({inference_api_key:'test-inference-key'})});
 assert.equal((await centralProxy(req('https://attacker.example'),env,fetcher,session)).status,403);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,fetcher,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,fetcher,session)).status,200);
 const saved=await centralProxy(new Request(env.APP_ORIGIN+path),env,fetcher,session);assert.equal(saved.status,200);assert.equal(saved.headers.get('cache-control'),'no-store');
});

test('admin signature routes retain session and origin protection',async()=>{
 const url=env.APP_ORIGIN+'/api/central/admin/capacity';
 const req=(origin:string)=>new Request(url,{method:'POST',headers:{origin},body:'{"challenge_id":"test","signature":"proof"}'});
 const transport:typeof fetch=async(url,options)=>{assert.match(String(url),/api\/v1\/admin\/capacity$/);assert.equal(new Headers(options?.headers).get('x-cc-user'),'verified-google-user');return Response.json({error:'Invalid admin signature'},{status:403});};
 assert.equal((await centralProxy(req('https://attacker.example'),env,transport,session)).status,403);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,transport,session)).status,403);
});

test('admin rechecks and provider credential custody require session and origin',async()=>{
 const paths=['admin/retest','projects/11111111-1111-4111-8111-111111111111/deployment/rpc-credential'];
 for(const path of paths){const req=(origin:string)=>new Request(env.APP_ORIGIN+'/api/central/'+path,{method:'POST',headers:{origin},body:'{}'});let called=0;const transport:typeof fetch=async(url)=>{called++;assert.equal(String(url),env.CENTRAL_API_ORIGIN+'/api/v1/'+path);return Response.json({authorized:true});};
 assert.equal((await centralProxy(req('https://attacker.example'),env,transport,session)).status,403);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,transport,session)).status,200);assert.equal(called,1);}
});

test('real launch directory uses the authenticated read-only backend route',async()=>{
 const transport:typeof fetch=async(url,options)=>{assert.equal(String(url),env.CENTRAL_API_ORIGIN+'/api/v1/launchpad');assert.equal(options?.method,'GET');return Response.json({launches:[],sales_enabled:false});};
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/launchpad'),env,transport,session)).status,200);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/launchpad'),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/launchpad',{method:'POST',headers:{origin:env.APP_ORIGIN},body:'{}'}),env,transport,session)).status,405);
});

test('real prompt trials require authenticated session and same-origin POST',async()=>{
 const path='launchpad/11111111-1111-4111-8111-111111111111/trial';const url=env.APP_ORIGIN+'/api/central/'+path;let calls=0;const transport:typeof fetch=async(target)=>{calls++;assert.equal(String(target),env.CENTRAL_API_ORIGIN+'/api/v1/'+path);return Response.json({text:'actual response'});};const req=(origin:string)=>new Request(url,{method:'POST',headers:{origin},body:'{}'});
 assert.equal((await centralProxy(req('https://attacker.example'),env,transport,session)).status,403);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(req(env.APP_ORIGIN),env,transport,session)).status,200);assert.equal(calls,1);
});

test('paid credit routes require authenticated sessions, same origin and exact methods',async()=>{
 const id='11111111-1111-4111-8111-111111111111';let calls=0;const transport:typeof fetch=async(url)=>{calls++;assert.match(String(url),/\/api\/v1\/credits\//);return Response.json({ok:true});};
 const request=(path:string,method='GET',origin=env.APP_ORIGIN)=>new Request(env.APP_ORIGIN+'/api/central/credits/'+path,{method,headers:{origin},...(method==='POST'?{body:'{}'}:{})});
 for(const action of ['claim-revenue','wallet/challenge','wallet/bind',id+'/transactions',id+'/purchase',id+'/infer',id+'/withdraw',id+'/deposit',id+'/complete-withdrawal',id+'/cancel-withdrawal']){
 assert.equal((await centralProxy(request(action,'POST','https://attacker.example'),env,transport,session)).status,403);
 assert.equal((await centralProxy(request(action,'POST'),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(request(action),env,transport,session)).status,405);
 assert.equal((await centralProxy(request(action,'POST'),env,transport,session)).status,200);
 }
 for(const path of ['me',id,id+'/request/'+'a'.repeat(64)]){assert.equal((await centralProxy(request(path),env,transport,session)).status,200);assert.equal((await centralProxy(request(path,'POST'),env,transport,session)).status,405);}
 assert.equal(calls,13);
 assert.equal((await centralProxy(request(id+'/settle','POST'),env,transport,session)).status,404);
});

test('collateral withdrawal preparation and tracking stay authenticated and same-origin',async()=>{
 const base='projects/11111111-1111-4111-8111-111111111111/collateral';let calls=0;
 const transport:typeof fetch=async(url)=>{calls++;assert.match(String(url),/\/collateral(?:\/(?:withdraw|submit))?$/);return Response.json({progressive:true});};
 const req=(path:string,method='GET',origin=env.APP_ORIGIN)=>new Request(env.APP_ORIGIN+'/api/central/'+path,{method,headers:{origin},...(method==='POST'?{body:'{}'}:{})});
 assert.equal((await centralProxy(req(base),env,transport,session)).status,200);
 assert.equal((await centralProxy(req(base),env,transport,async()=>Response.json({}))).status,401);
 for(const action of ['withdraw','submit']){assert.equal((await centralProxy(req(base+'/'+action,'POST','https://attacker.example'),env,transport,session)).status,403);assert.equal((await centralProxy(req(base+'/'+action),env,transport,session)).status,405);assert.equal((await centralProxy(req(base+'/'+action,'POST'),env,transport,session)).status,200);}
 assert.equal(calls,3);
});


test('protocol readiness is session-protected same-origin POST with no activation route',async()=>{
 const url=env.APP_ORIGIN+'/api/central/admin/protocol';let calls=0;
 const transport:typeof fetch=async(target)=>{calls++;assert.equal(String(target),env.CENTRAL_API_ORIGIN+'/api/v1/admin/protocol');return Response.json({can_activate:false});};
 const req=(method:string,origin=env.APP_ORIGIN)=>new Request(url,{method,headers:{origin},...(method==='POST'?{body:'{}'}:{})});
 assert.equal((await centralProxy(req('POST','https://attacker.example'),env,transport,session)).status,403);
 assert.equal((await centralProxy(req('POST'),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(req('GET'),env,transport,session)).status,405);
 assert.equal((await centralProxy(req('POST'),env,transport,session)).status,200);
 assert.equal(calls,1);
 assert.equal((await centralProxy(new Request(env.APP_ORIGIN+'/api/central/admin/activate',{method:'POST',headers:{origin:env.APP_ORIGIN},body:'{}'}),env,transport,session)).status,404);
});

test('modular ledger and migration consent routes enforce authentication and exact methods',async()=>{
 const id='11111111-1111-4111-8111-111111111111';const make=(action:string,method='POST',origin=env.APP_ORIGIN)=>new Request(env.APP_ORIGIN+'/api/central/modular/'+id+(action?'/'+action:''),{method,headers:{origin},...(method==='POST'?{body:'{}'}:{})});let calls=0;const transport:typeof fetch=async()=>{calls++;return Response.json({ok:true});};
 for(const action of ['transactions','purchase','reserve','infer','withdraw','deposit','claim-revenue','release','consent']){
  assert.equal((await centralProxy(make(action),env,transport,session)).status,200);
  assert.equal((await centralProxy(make(action,'GET'),env,transport,session)).status,405);
  assert.equal((await centralProxy(make(action,'POST','https://attacker.example'),env,transport,session)).status,403);
 }
 for(const action of ['','info','consent-plan']){assert.equal((await centralProxy(make(action,'GET'),env,transport,session)).status,200);assert.equal((await centralProxy(make(action),env,transport,session)).status,405);}
 assert.equal((await centralProxy(make('reserve'),env,transport,async()=>Response.json({}))).status,401);
 assert.equal((await centralProxy(make('execute'),env,transport,session)).status,404);assert.equal(calls,12);
});

test('migration progress polling requires session and same-origin POST',async()=>{
 const path=env.APP_ORIGIN+'/api/central/admin/migration-status';let calls=0;
 const transport:typeof fetch=async(target)=>{calls++;assert.equal(String(target),env.CENTRAL_API_ORIGIN+'/api/v1/admin/migration-status');return Response.json({stage:'timelock_countdown'});};
 const req=(method='POST',origin=env.APP_ORIGIN)=>new Request(path,{method,headers:{origin},...(method==='POST'?{body:'{}'}:{})});
 assert.equal((await centralProxy(req(),env,transport,session)).status,200);
 assert.equal((await centralProxy(req('GET'),env,transport,session)).status,405);
 assert.equal((await centralProxy(req('POST','https://attacker.example'),env,transport,session)).status,403);
 assert.equal((await centralProxy(req(),env,transport,async()=>Response.json({}))).status,401);
 assert.equal(calls,1);
});
