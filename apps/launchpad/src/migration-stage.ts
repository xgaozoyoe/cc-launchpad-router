export function canScheduleMigration(status:{stage:string;deployment_verified:boolean}|null,pending:boolean,error:boolean):boolean{return status?.deployment_verified===true&&status.stage==='not_scheduled'&&!pending&&!error;}
export function migrationChecklist(status:{stage:string;deployment_verified?:boolean;monitor_ready?:boolean;asset_progress?:{plan_ready:boolean;funding_ready:boolean;commit_state:string}}|null,phase?:string){
 const transferred=['mined_waiting_finality','finalized'].includes(status?.asset_progress?.commit_state??'');
 const active=phase==='active',frozen=phase==='frozen'||active||transferred,stage=status?.stage;
 const scheduled=active||transferred||['timelock_countdown','ready_to_execute','gate_binding_waiting_finality','gate_bound_finalized','executed_waiting_binding'].includes(stage??'');
 const bound=active||transferred||stage==='gate_bound_finalized';
 const platform=active||transferred||!!status?.monitor_ready;
 const consent=active||transferred||!!(status?.asset_progress?.plan_ready&&status.asset_progress.funding_ready);
 const committed=active||status?.asset_progress?.commit_state==='finalized';
 const completed=[scheduled,bound,platform,frozen,consent,committed,active];
 const current=completed.findIndex(done=>!done);
 return completed.map((done,index)=>done?'done':index===current?'current':'pending') as ('done'|'current'|'pending')[];
}
