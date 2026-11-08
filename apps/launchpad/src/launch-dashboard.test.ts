import {test} from 'node:test';
import assert from 'node:assert/strict';
import {filterLaunches,launchGroup,stakeUnits,stakeBalances,launchProgress,projectBadge,projectAction} from './launch-dashboard.ts';
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

test('checklist advances to capacity only after recorded API pass and keeps sales incomplete',()=>{
 const project={...projects[1],stake_finalized:true,progress:{bundle_issued:true,last_proxy_at:123,rpc_status:'passed',rpc_checked_at:456}};
 const steps=launchProgress(project);assert.deepEqual(steps.map(s=>s.state),['complete','complete','complete','complete','current','upcoming']);assert.equal(projectBadge(project),'Awaiting capacity acceptance');assert.equal(projectAction(project),'View verification results');
 const failed=launchProgress({...project,progress:{...project.progress,rpc_status:'failed'}});assert.equal(failed[3].state,'current');assert.equal(failed[4].state,'upcoming');
 const expired=launchProgress({...project,status:'expired'});assert.equal(expired.some(s=>s.state==='current'),false);assert.equal(expired[4].state,'blocked');
 const draft=launchProgress(projects[0]);assert.equal(draft[0].state,'complete');assert.equal(draft[1].state,'current');assert.equal(draft[2].state,'upcoming');
});

test('accepted is setup-complete but never sales live',()=>{
 const p={...projects[1],status:'accepted',stake_finalized:true,progress:{bundle_issued:true,last_proxy_at:123,rpc_status:'passed'}};
 assert.equal(launchGroup(p.status),'rpc');assert.match(projectBadge(p),/sales closed/);assert.equal(launchProgress(p)[4].state,'complete');assert.equal(launchProgress(p)[5].state,'current');
});

test('capacity approval advances the badge to on-chain verification without claiming acceptance',()=>{
 const p={...projects[1],capacity_approval:{wallet:'0xadmin',at:123,reason:'Demo approval',type:'manual-demo-not-benchmarked'},progress:{bundle_issued:true,rpc_status:'passed'}};
 assert.equal(projectBadge(p),'Capacity approved · awaiting on-chain verification');
 assert.equal(projectBadge({...p,status:'verification-pending'}),'Verification pending finality');
 assert.equal(projectBadge({...p,status:'accepted'}),'Accepted · sales closed');
});
