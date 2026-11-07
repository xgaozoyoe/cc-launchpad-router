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
