import { test } from 'node:test';
import assert from 'node:assert/strict';
import { configured, emailAllowed, handleAuth, validOrigin } from './auth.ts';
import type { AuthEnvironment } from './auth.ts';
const env:AuthEnvironment={GOOGLE_CLIENT_ID:'test-client.apps.googleusercontent.com',GOOGLE_CLIENT_SECRET:'test-only-not-a-real-client-secret',AUTH_SECRET:'a-test-only-secret-of-at-least-32-characters',APP_ORIGIN:'https://launchpad.example',AUTH_ALLOWED_EMAILS:'owner@example.com'};

test('missing credentials fail closed and status never exposes secrets',async()=>{
  assert.equal(configured({}),false);
  const status=await handleAuth(new Request('https://launchpad.example/api/auth/status'),env);
  assert.deepEqual(await status.json(),{configured:true});
  const response=await handleAuth(new Request('https://launchpad.example/api/auth/session'),{});
  assert.equal(response.status,503);
  assert.equal(response.headers.get('cache-control'),'no-store');
});
test('private demo requires a verified Google email on the allowlist',()=>{
  assert.equal(emailAllowed('OWNER@example.com',true,env.AUTH_ALLOWED_EMAILS),true);
  assert.equal(emailAllowed('someone@example.com',true,env.AUTH_ALLOWED_EMAILS),false);
  assert.equal(emailAllowed('owner@example.com',false,env.AUTH_ALLOWED_EMAILS),false);
  assert.equal(emailAllowed('owner@example.com',true,''),false);
  assert.equal(configured({...env,AUTH_ALLOWED_EMAILS:''}),false);
});
test('only secure production origins and explicit local development origins are valid',()=>{
  assert.equal(validOrigin('http://localhost:4173'),'http://localhost:4173');
  assert.equal(validOrigin('https://launchpad.example'),'https://launchpad.example');
  for(const value of ['http://launchpad.example','https://user:password@example.com','https://example.com/path','https://example.com?x=1','not-a-url'])assert.equal(validOrigin(value),null);
});
test('public mode accepts verified Google accounts without an email allowlist',()=>{
  assert.equal(configured({...env,AUTH_ACCESS_MODE:'public',AUTH_ALLOWED_EMAILS:''}),true);
  assert.equal(emailAllowed('new-user@example.com',true,undefined,'public'),true);
  assert.equal(emailAllowed('new-user@example.com',false,undefined,'public'),false);
  assert.equal(emailAllowed(undefined,true,undefined,'public'),false);
  assert.equal(emailAllowed('new-user@example.com',true,undefined,'unknown'),false);
});
test('cross-origin POST requests cannot start or end a session',async()=>{
  for(const origin of ['https://attacker.example','null','']){
    const response=await handleAuth(new Request('https://launchpad.example/api/auth/signout',{method:'POST',headers:{origin}}),env);
    assert.equal(response.status,403);
  }
});
test('an unsigned anonymous visitor receives no authenticated identity',async()=>{
  const response=await handleAuth(new Request('https://launchpad.example/api/auth/session'),env);
  assert.equal(response.status,200);
  assert.equal(await response.json(),null);
});
test('CSRF endpoint issues a non-cacheable cookie for the OAuth flow',async()=>{
  const response=await handleAuth(new Request('https://launchpad.example/api/auth/csrf'),env);
  const result=await response.json() as {csrfToken:string};
  assert.equal(response.status,200);
  assert.ok(result.csrfToken.length>=32);
  const cookies=response.headers.getSetCookie().join(';');
  assert.match(cookies,/HttpOnly/i);
  assert.match(cookies,/Secure/i);
  assert.match(cookies,/SameSite=Lax/i);
});

test('Google account identity stays stable and links only a matching verified legacy session',async()=>{
  const {authConfig}=await import('./auth.ts');
  const legacy={sub:'11111111-1111-4111-8111-111111111111',email:'owner@example.com'};
  const links:string[][]=[];
  const callback=authConfig(env,legacy,async(_env,next,old)=>{links.push([next,old]);}).callbacks!.jwt!;
  const input={token:{sub:'temporary-login-id'},user:{id:'temporary-login-id'},account:{provider:'google',providerAccountId:'123456789',type:'oidc' as const},profile:{sub:'123456789',email:'owner@example.com',email_verified:true},trigger:'signIn' as const,isNewUser:false};
  assert.equal((await callback(input))?.sub,'google:123456789');
  assert.deepEqual(links,[['google:123456789',legacy.sub]]);
  const other=authConfig(env,{...legacy,email:'different@example.com'},async()=>{throw new Error('Must not link another account');}).callbacks!.jwt!;
  assert.equal((await other({...input,token:{sub:'another-login-id'}}))?.sub,'google:123456789');
  const failure=authConfig(env,legacy,async()=>{throw new Error('Migration unavailable');}).callbacks!.jwt!;
  await assert.rejects(async()=>failure(input),/Migration unavailable/);
});
