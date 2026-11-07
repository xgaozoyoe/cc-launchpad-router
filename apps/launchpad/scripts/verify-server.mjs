import assert from 'node:assert/strict';
import handler from '../.server-check/api/auth.js';
const headers=new Map();
let body='';
const response={statusCode:0,setHeader(key,value){headers.set(key.toLowerCase(),value);},end(value){body=value.toString();}};
await handler({url:'/api/auth/status',method:'GET',headers:{}},response,{});
assert.equal(response.statusCode,200);
assert.deepEqual(JSON.parse(body),{configured:false});
assert.equal(headers.get('cache-control'),'no-store');
console.log('Compiled Node ESM authentication endpoint passed.');

// Production must fail before opening a listener if test-account mode leaks in.
const {spawnSync}=await import('node:child_process');
const guarded=spawnSync(process.execPath,['.server-check/server/standalone.js'],{env:{PATH:process.env.PATH,APP_ORIGIN:'http://localhost:4174',LOCAL_TEST_ACCOUNT:'1',NODE_ENV:'production'},encoding:'utf8'});
assert.notEqual(guarded.status,0);
assert.match(guarded.stderr,/Local test account is restricted/);
console.log('Production local-account guard passed.');
