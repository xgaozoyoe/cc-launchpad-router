import { test } from 'node:test';
import assert from 'node:assert/strict';
import { initialState } from './initial.ts';
import { demoReducer, parseSavedState } from './state.ts';
import type { DemoState, DemoAction } from './types';
function issue(s:DemoState,a:Extract<DemoAction,{type:'launch'}>){
 let state=demoReducer(s,{...a,type:'project-create'});
 if(state.notice?.error)return state;
 state=demoReducer(state,{type:'project-stake',id:'stake-'+a.id,projectId:a.id,txHash:'0x'+'a'.repeat(64),blockTimestamp:1000,now:1100,launchId:a.id});
 if(state.notice?.error)return state;
 return demoReducer(state,{type:'project-verify',id:'verify-'+a.id,projectId:a.id,now:1200});
}
import { offersForModel } from './models.ts';

test('purchase transfers demo funds into a model-specific credit entitlement',()=>{
  const initial=initialState();
  const next=demoReducer(initial,{type:'buy',id:'purchase-1',launchId:'atlas',quantity:1e6});
  assert.equal(next.wallet,247.6);
  assert.equal(next.credits.atlas,3400000);
  assert.equal(next.launches[0].sold,69000000);
  assert.equal(initial.credits.atlas,2400000);
  assert.equal(initial.wallet,250);
});
test('unaffordable and oversold purchases cannot alter balances',()=>{
  const s=initialState();
  for(const quantity of [1e9,32000001,1000.5,-1000]){
    const next=demoReducer(s,{type:'buy',id:'bad',launchId:'atlas',quantity});
    assert.equal(next.wallet,s.wallet);
    assert.deepEqual(next.credits,s.credits);
    assert.equal(next.notice?.error,true);
  }
  const emptyWallet={...s,wallet:0};
  assert.equal(demoReducer(emptyWallet,{type:'buy',id:'broke',launchId:'atlas',quantity:1000}).notice?.error,true);
});
test('redemption burns accepted credits once, even if settlement is retried',()=>{
  const s=initialState();
  const action={type:'redeem' as const,id:'request-1',launchId:'atlas',tokens:12000,outage:false};
  const next=demoReducer(s,action);
  assert.equal(next.credits.atlas,s.credits.atlas-12000);
  assert.equal(next.burned,12000);
  assert.equal(next.wallet,s.wallet);
  assert.equal(next.launches[0].stake,s.launches[0].stake);
  assert.equal(demoReducer(next,action),next);
});
test('fallback penalty covers the incremental cost without charging the buyer twice',()=>{
  const s=initialState();
  const next=demoReducer(s,{type:'redeem',id:'outage-1',launchId:'atlas',tokens:1000000,outage:true});
  assert.equal(next.lastRequest?.extra,1.2);
  assert.equal(next.lastRequest?.slash,.6);
  assert.equal(next.launches[0].stake,7999.4);
  assert.equal(next.reserve,s.reserve);
  assert.equal(next.wallet,s.wallet);
});
test('insufficient collateral leaves a reserve loss without a negative PRL balance',()=>{
  const s=initialState();s.launches[0].stake=.1;
  const next=demoReducer(s,{type:'redeem',id:'shortfall',launchId:'atlas',tokens:1000000,outage:true});
  assert.equal(next.launches[0].stake,0);
  assert.equal(next.lastRequest?.slash,.1);
  assert.ok(Math.abs(next.reserve-(s.reserve-1))<1e-8);
});
test('unavailable protection and insufficient credits preserve unspent entitlements',()=>{
  const base=initialState();
  for(const s of [{...base,reserve:0},{...base,poolCapacity:0},{...base,credits:{atlas:1}}]){
    const next=demoReducer(s,{type:'redeem',id:'unserved',launchId:'atlas',tokens:12000,outage:true});
    assert.equal(next.notice?.error,true);
    assert.deepEqual(next.credits,s.credits);
    assert.equal(next.burned,0);
    assert.equal(next.receipts.length,0);
  }
});
test('launch issuance requires verification and respects the collateral cap',()=>{
  const s=initialState(),action={type:'launch' as const,id:'new-launch',name:'Test launch',modelId:'atlas-8b',family:'Llama' as const,price:2.4,total:100000000,stake:1000,window:'Dec 2026',verified:false};
  assert.equal(demoReducer(s,action).notice?.error,true);
  assert.equal(demoReducer(s,{...action,verified:true}).notice?.error,true);
  const next=issue(s,{...action,verified:true,total:10000000});
  assert.equal(next.launches.length,s.launches.length+1);
  assert.equal(next.launches.at(-1)!.total,10000000);
});
test('versioned storage imports the original demo and rejects corrupt domain data',()=>{
  const original=initialState();
  const legacy={...original,version:undefined,notice:undefined};
  assert.equal(parseSavedState(JSON.stringify(legacy))?.credits.atlas,2400000);
  assert.equal(parseSavedState('{invalid'),null);
  assert.equal(parseSavedState(JSON.stringify({...original,credits:{atlas:-1}})),null);
  assert.equal(parseSavedState(JSON.stringify({...original,launches:[{...original.launches[0],sold:1e20}]})),null);
});
test('PRL deposits and withdrawals update available funds once',()=>{
  const start=initialState();
  const deposit={type:'prl-transfer' as const,id:'deposit-1',kind:'deposit' as const,amount:3000,at:'2026-11-04T00:00:00Z'};
  const funded=demoReducer(start,deposit);
  assert.equal(funded.prlAvailable,13000);
  assert.equal(demoReducer(funded,deposit),funded);
  const withdrawn=demoReducer(funded,{...deposit,id:'withdraw-1',kind:'withdrawal',amount:4000});
  assert.equal(withdrawn.prlAvailable,9000);
  assert.equal(withdrawn.prlTransfers.length,2);
  assert.equal(start.prlAvailable,10000);
});
test('launching locks available PRL and prevents withdrawing collateral',()=>{
  const start=initialState();
  const action={type:'launch' as const,id:'locked-launch',name:'Funded launch',modelId:'atlas-8b',family:'Llama' as const,price:2.4,total:10000000,stake:6000,window:'Dec 2026',verified:true};
  const locked=issue(start,action);
  assert.equal(locked.prlAvailable,4000);
  assert.equal(locked.launches.filter(l=>l.own).reduce((a,l)=>a+l.stake,0),6000);
  assert.equal(demoReducer(locked,{type:'project-verify',id:'repeat',projectId:action.id,now:1200}),locked);
  const denied=demoReducer(locked,{type:'prl-transfer',id:'too-large',kind:'withdrawal',amount:5000,at:'2026-11-04T00:00:00Z'});
  assert.equal(denied.notice?.error,true);
  assert.equal(denied.prlAvailable,4000);
  assert.equal(denied.prlTransfers.length,0);
  const unfunded=issue({...start,prlAvailable:0},action);
  assert.equal(unfunded.notice?.error,true);
  assert.equal(unfunded.launches.length,start.launches.length);
});
test('saved accounts retain PRL funds and legacy accounts receive a demo starter balance',()=>{
  const state=initialState();
  assert.equal(parseSavedState(JSON.stringify({...state,prlAvailable:3210}))?.prlAvailable,3210);
  assert.equal(parseSavedState(JSON.stringify({...state,prlAvailable:undefined,prlTransfers:undefined}))?.prlAvailable,10000);
  assert.equal(parseSavedState(JSON.stringify({...state,prlAvailable:-1})),null);
  assert.equal(parseSavedState(JSON.stringify({...state,prlTransfers:[{id:'bad',kind:'deposit',amount:-10,at:'now'}]})),null);
});
test('credits stay separate by launch and cannot cover another model’s shortfall',()=>{
  const start=initialState();
  const purchased=demoReducer(start,{type:'buy',id:'buy-nova',launchId:'nova',quantity:1000000});
  const used=demoReducer(purchased,{type:'redeem',id:'use-atlas',launchId:'atlas',tokens:12000,outage:false});
  assert.equal(used.credits.atlas,start.credits.atlas-12000);
  assert.equal(used.credits.nova,1000000);
  const limited={...used,credits:{...used.credits,atlas:10}};
  const denied=demoReducer(limited,{type:'redeem',id:'no-cross-model-spend',launchId:'atlas',tokens:12000,outage:false});
  assert.equal(denied.notice?.error,true);
  assert.deepEqual(denied.credits,limited.credits);
});
test('model comparison returns only matching available offers in ascending unit price',()=>{
  const state=initialState();
  const offers=offersForModel(state.launches,'atlas-8b');
  assert.deepEqual(offers.map(l=>l.price),[2.1,2.4]);
  assert.equal(new Set(offers.map(l=>l.supplier)).size,2);
  assert.ok(offers.every(l=>l.modelId==='atlas-8b'));
  const soldOut=state.launches.map(l=>l.id==='atlas-greengrid'?{...l,sold:l.total}:l);
  assert.deepEqual(offersForModel(soldOut,'atlas-8b').map(l=>l.id),['atlas']);
  assert.equal(state.launches[0].id,'atlas');
});
test('credit launches use the selected registry model instead of an arbitrary family',()=>{
  const state=initialState();
  const action={type:'launch' as const,id:'qwen-launch',name:'My custom provider credits',modelId:'nova-14b',family:'Llama' as const,price:3.9,total:10000000,stake:1000,window:'Dec 2026',verified:true};
  const next=issue(state,action);
  const launch=next.launches.at(-1)!;
  assert.equal(launch.name,action.name);
  assert.equal(launch.modelId,'nova-14b');
  assert.equal(launch.family,'Qwen');
  assert.equal(launch.precision,'BF16');
  assert.equal(launch.context,'64K context');
  const denied=issue(state,{...action,modelId:'unsupported-model'});
  assert.equal(denied.notice?.error,true);
  assert.equal(denied.prlAvailable,state.prlAvailable);
});

test('project waits for stake and RPC before sales; deadline uses block time, not confirmation',()=>{
 const start=initialState();
 const draft=demoReducer(start,{type:'project-create',id:'draft',name:'Test',modelId:'qwen3-8b-awq',stake:2000,price:2,total:10000000,window:'Dec 2026'});
 assert.equal(draft.prlAvailable,start.prlAvailable);assert.equal(draft.projects[0].launchId,undefined);
 const stake={type:'project-stake' as const,id:'s',projectId:'draft',txHash:'0x'+'b'.repeat(64),blockTimestamp:1000,now:2000,launchId:'launch-t'};
 const pending=demoReducer(draft,stake);
 assert.equal(pending.projects[0].providerId,stake.txHash);assert.equal(pending.projects[0].deadline,87400);
 assert.equal(pending.launches.length,start.launches.length);assert.equal(pending.prlAvailable,8000);
 assert.equal(demoReducer(pending,stake),pending);
 const expired=demoReducer(pending,{type:'project-verify',id:'late',projectId:'draft',now:87400});
 assert.equal(expired.notice?.error,true);assert.equal(expired.launches.length,start.launches.length);
 const accepted=demoReducer(pending,{type:'project-verify',id:'ok',projectId:'draft',now:87399});
 assert.equal(accepted.launches.at(-1)?.id,'launch-t');assert.equal(accepted.prlAvailable,8000);
 assert.equal(parseSavedState(JSON.stringify(accepted))?.projects[0].status,'live');
});
