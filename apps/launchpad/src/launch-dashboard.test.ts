import {test} from 'node:test';
import assert from 'node:assert/strict';
import {filterLaunches,launchGroup,stakeUnits,stakeBalances} from './launch-dashboard.ts';
const projects=[{_id:'draft-1',name:'First model',model_id:'qwen3-8b-awq',status:'draft'},{_id:'ready-2',name:'Second model',model_id:'qwen3-8b-awq',status:'awaiting-rpc',launch_id:'launch-abc'}];
test('submitted stakes are pending, not accepted RPC or live sales',()=>{assert.equal(launchGroup('stake-submitted'),'staking');assert.equal(launchGroup('awaiting-rpc'),'rpc');assert.equal(launchGroup('expired'),'inactive');assert.equal(launchGroup('future-status'),'other');});
test('stage and case-insensitive identity search intersect',()=>{assert.deepEqual(filterLaunches(projects,'rpc',' LAUNCH-ABC '),[projects[1]]);assert.equal(filterLaunches(projects,'draft','launch-abc').length,0);assert.equal(filterLaunches(projects,'all','qwen3').length,2);});
test('stake balance avoids floating point overflow',()=>{assert.equal(stakeUnits('2000000000000000000000'),'2,000 tPRL');assert.equal(stakeUnits(),'Not committed');assert.equal(stakeUnits('invalid'),'Unavailable');});

test('only recorded deposits count as locked; free is never invented from delivery',()=>{
 const project={...projects[1],stake_raw:'2000000000000000000000',stake_finalized:true};
 assert.equal(stakeBalances(project)?.locked,2000n*10n**18n);
 assert.equal(stakeBalances(project)?.free,0n);
 assert.equal(stakeBalances({...project,status:'expired'})?.locked,2000n*10n**18n);
 assert.equal(stakeBalances({...project,status:'refunded'})?.locked,0n);
 assert.equal(stakeBalances({...project,status:'released'})?.returned,2000n*10n**18n);
 assert.equal(stakeBalances({...project,stake_finalized:false}),null);
});
