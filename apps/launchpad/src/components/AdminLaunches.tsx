import {approvalBlocker,reviewCategory} from '../admin-review';
import {DeploymentMigration} from './DeploymentMigration';
import {MigrationControls} from './MigrationControls';
import type {MigrationStatus} from './MigrationControls';
import {useCallback,useEffect,useRef,useState} from 'react';
import {ProtocolCutover} from './ProtocolCutover';
import {serverPost} from '../server-projects';
import type {ServerProject} from '../server-projects';
import {ensureWalletChain} from '../wallet-network';
import {projectBadge} from '../launch-dashboard';
type Wallet={request:(args:{method:string;params?:unknown[]})=>Promise<unknown>;on?:(event:string,fn:()=>void)=>void;removeListener?:(event:string,fn:()=>void)=>void};
type ReviewProject=ServerProject&{capacity_approval?:{wallet:string;at:number;reason:string;type:string};rpc_retest_available?:boolean;verification_tx_hash?:string;migration_holders?:string[];migration_available_group?:string;migration_stake_state?:number;migration_prl_principal_wei?:string;migration_credit_issued?:string;migration_credit_commitment?:string;migration_inventory_error?:string};
type Listing={projects:ReviewProject[];admin_address:string;chain_id:number;contract:string;credit_contract?:string;candidate_groups?:string[]};
const provider=()=>{const p=(window as unknown as {ethereum?:Wallet}).ethereum;if(!p)throw new Error('Open this page in a browser with an Ethereum wallet extension.');return p;};
const messageHex=(message:string)=>'0x'+Array.from(new TextEncoder().encode(message),b=>b.toString(16).padStart(2,'0')).join('');
export function AdminLaunches(){
 const [category,setCategory]=useState<'migration'|'approvals'>('migration');
 const [wallet,setWallet]=useState(''),[token,setToken]=useState(''),[listing,setListing]=useState<Listing|null>(null),[busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const epoch=useRef(0),switchingNetwork=useRef(false);
 const now=Date.now()/1000;
 const queues={ready:listing?.projects.filter(p=>reviewCategory(p,now)==='ready')??[],waiting:listing?.projects.filter(p=>reviewCategory(p,now)==='waiting')??[],pending:listing?.projects.filter(p=>reviewCategory(p,now)==='pending')??[],accepted:listing?.projects.filter(p=>reviewCategory(p,now)==='accepted')??[]};
 useEffect(()=>{
  if(!token||busy)return;
  const generation=epoch.current;let active=true,running=false;
  const timer=setInterval(()=>{if(running)return;running=true;void serverPost<Listing>('admin/query',{token}).then(data=>{if(active&&epoch.current===generation)setListing(data);}).catch(e=>{if(active){setError(e instanceof Error?e.message:'Could not refresh verification progress');clearInterval(timer);}}).finally(()=>{running=false;});},10000);
  return()=>{active=false;clearInterval(timer);};
 },[token,busy]);
 useEffect(()=>{const p=(window as unknown as {ethereum?:Wallet}).ethereum;const clear=()=>{epoch.current++;setWallet('');setToken('');setListing(null);setNotice('Wallet changed. Connect and sign again.');};const chainChanged=()=>{if(!switchingNetwork.current)clear();};p?.on?.('accountsChanged',clear);p?.on?.('chainChanged',chainChanged);return()=>{epoch.current++;p?.removeListener?.('accountsChanged',clear);p?.removeListener?.('chainChanged',chainChanged);};},[]);
 async function signed<T>(intent:string,payload:unknown,address:string,progress?:(message:string)=>void){
  progress?.("Preparing an account-bound authorization challenge…");
  const beforeChallenge=epoch.current;
  const challenge=await serverPost<{challenge_id:string;message:string;admin_address:string;chain_id:number}>('admin/challenge',{intent,payload});
  if(address.toLowerCase()!==challenge.admin_address.toLowerCase())throw new Error('The connected wallet is not the configured admin address: '+challenge.admin_address);
  if(epoch.current!==beforeChallenge)throw new Error('Wallet changed. Connect again.');
  if(intent==='login'){
   switchingNetwork.current=true;
   try{await ensureWalletChain(provider(),challenge.chain_id);}finally{switchingNetwork.current=false;}
   const accounts=await provider().request({method:'eth_accounts'});
   if(!Array.isArray(accounts)||typeof accounts[0]!=='string'||accounts[0].toLowerCase()!==address.toLowerCase()||epoch.current!==beforeChallenge)throw new Error('Wallet account changed. Connect again.');
  }
  const started=epoch.current;
  progress?.("Waiting for your wallet authorization signature. No transaction has been sent.");
  const signature=await provider().request({method:'personal_sign',params:[messageHex(challenge.message),address]});
  if(epoch.current!==started)throw new Error('Wallet changed. Reconnect before continuing.');
  progress?.('Signature received · waiting for server verification…');
  const result=await serverPost<T>('admin/'+(intent==='login'?'authenticate':intent),{challenge_id:challenge.challenge_id,signature});
  if(epoch.current!==started)throw new Error('Wallet changed. Reconnect before continuing.');return result;
 }
 const migrationStatus=useCallback((group:string,hash?:string)=>serverPost<MigrationStatus>('admin/migration-status',{token,group_id:group,...(hash?{tx_hash:hash}:{})}),[token]);
 async function refresh(access=token){const started=epoch.current;const data=await serverPost<Listing>('admin/query',{token:access});if(epoch.current===started)setListing(data);}
 async function run(label:string,fn:()=>Promise<void>){setBusy(label);setError('');setNotice('');try{await fn();}catch(e){setError(e instanceof Error?e.message:'Request failed');}finally{setBusy('');}}
 async function connect(){
  const accounts=await provider().request({method:'eth_requestAccounts'});
  if(!Array.isArray(accounts)||typeof accounts[0]!=='string')throw new Error('No wallet selected');
  const address=accounts[0].toLowerCase();
  const auth=await signed<{token:string}>('login',{},address);setWallet(address);setToken(auth.token);await refresh(auth.token);
 }
 async function retest(project:ReviewProject){
  const result=await signed<{report:{status:string;failure?:string}}>('retest',{project_id:project._id},wallet);
  await refresh();setNotice(result.report.status==='passed'?'RPC recheck passed. You can continue approval.':'RPC recheck failed: '+(result.report.failure??'See the project result.'));
 }
 async function approve(project:ReviewProject,reason:string){await signed('capacity',{project_id:project._id,reason,manual_demo:true},wallet);await refresh();setNotice('Manual capacity approval recorded. No benchmark was performed.');}
 async function verify(project:ReviewProject){
  if(!listing)throw new Error('Connect again');
  const chain=await provider().request({method:'eth_chainId'});
  if(chain!=='0x'+listing.chain_id.toString(16))throw new Error('Switch your wallet to the configured network (chain '+listing.chain_id+'), then reconnect.');
  const ready=await signed<{transaction:Record<string,string>}>('prepare',{project_id:project._id},wallet);
  const finalChain=await provider().request({method:'eth_chainId'});
  const accounts=await provider().request({method:'eth_accounts'});
  if(finalChain!=='0x'+listing.chain_id.toString(16)||!Array.isArray(accounts)||typeof accounts[0]!=='string'||accounts[0].toLowerCase()!==wallet.toLowerCase())throw new Error('Wallet account or network changed. Reconnect before sending the transaction.');
  setNotice('Confirm verifyProject in your wallet. This uses test ETH for gas; credit sales remain closed.');
  const tx=await provider().request({method:'eth_sendTransaction',params:[ready.transaction]});
  if(typeof tx!=='string')throw new Error('Wallet did not return a transaction hash');
  setNotice('Transaction sent: '+tx+'. Waiting for finalized on-chain acceptance.');
  try{await signed('submit',{project_id:project._id,tx_hash:tx},wallet);}catch(e){throw new Error('Transaction sent: '+tx+'. Tracking failed: '+(e instanceof Error?e.message:'retry refresh')+'. Refresh to reconcile chain state; do not send a second transaction.');}
  await refresh();
 }
 const renderCard=(project:ReviewProject)=><ReviewCard project={project} adminWallet={listing?.admin_address??''} onRetest={()=>void run('Running signed RPC recheck',()=>retest(project))} busy={!!busy} onApprove={reason=>void run('Signing capacity approval',()=>approve(project,reason))} onVerify={()=>void run('Signing on-chain verification',()=>verify(project))}/>;
 return <section className="admin-page"><div className="page-heading"><div><span className="eyebrow">PLATFORM ADMINISTRATION</span><h1>Administration</h1><p>Deployment migration and model-launch approval are separate workflows.</p></div></div><div className="setup-note"><strong>Wallet authorization required</strong><p>The server verifies a fresh signature for protected actions. Google sign-in alone grants no admin authority. Confirm the configured network in your wallet.</p><button className="primary-button" disabled={!!busy} onClick={()=>void run('Connecting wallet',connect)}>{token?'Reconnect admin wallet':'Connect admin wallet & sign'}</button>{wallet?<p style={{overflowWrap:'anywhere'}}>Connected: {wallet}</p>:null}</div>{error?<p className="error" role="alert">{error}</p>:null}{notice?<p role="status" style={{overflowWrap:'anywhere'}}>{notice}</p>:null}{busy?<p role="status">{busy}…</p>:null}<nav className="admin-categories" aria-label="Administration category"><button className={category==='migration'?'primary-button':'outline-button'} aria-pressed={category==='migration'} onClick={()=>setCategory('migration')}>Deployment migration</button><button className={category==='approvals'?'primary-button':'outline-button'} aria-pressed={category==='approvals'} onClick={()=>setCategory('approvals')}>Waiting for approval ({queues.ready.length})</button></nav>{listing&&category==='migration'?<DeploymentMigration projects={listing.projects} sourceStake={listing.contract} sourceCredits={listing.credit_contract} targetGroup={listing.candidate_groups?.join(', ')} readiness={<ProtocolCutover token={token}/>}>{(()=>{const project=listing.projects.find(p=>p.migration_available_group);return project?<MigrationControls projectName={project.name} projectId={project._id} phase={project.migration_phase} group={project.migration_available_group} busy={!!busy} holders={project.migration_holders} onStatus={migrationStatus} onAction={async(operation,payload,progress)=>{let output:{tx_hash?:string}={};setBusy('Migration operation in progress');setError('');try{
   const ready=await signed<{transaction?:Record<string,string>;gate?:string;governance?:string;operation_id?:string;delay_seconds?:number;plan_hash?:string}>(operation,payload,wallet,progress);
   if(operation==='migration-prepare'||operation==='migration-governance'){
    const target=operation==='migration-governance'?ready.governance:ready.gate;
    if(!ready.transaction||!target||ready.transaction.to?.toLowerCase()!==target.toLowerCase()||ready.transaction.from?.toLowerCase()!==wallet.toLowerCase()||ready.transaction.chainId!=='0x'+listing.chain_id.toString(16)||ready.transaction.value!=='0x0')throw new Error('Prepared migration does not match the admin wallet and target gate');
    if(await provider().request({method:'eth_chainId'})!==ready.transaction.chainId)throw new Error('Switch to the configured chain first');
    const accounts=await provider().request({method:'eth_accounts'});if(!Array.isArray(accounts)||accounts[0]?.toLowerCase()!==wallet.toLowerCase())throw new Error('Admin wallet changed');
    setNotice(operation==='migration-governance'?'Review the fixed gate-binding governance operation in your wallet. The contract delay is '+ready.delay_seconds+' seconds; this does not move legacy funds.':'Review the atomic migration in your wallet. This retires old credits and moves PRL; the finalized commit is irreversible.');
    progress?.('Authorization verified · confirm the separate transaction in your wallet. No transaction hash yet.');
    const hash=await provider().request({method:'eth_sendTransaction',params:[ready.transaction]});if(typeof hash!=='string')throw new Error('Wallet did not return a transaction hash');output={tx_hash:hash};try{localStorage.setItem((operation==='migration-prepare'?'migration-tx:':'migration-governance-tx:')+project._id,hash);}catch{/* The hash remains visible below. */}setNotice(operation==='migration-governance'?'Governance transaction submitted: '+hash+'. The '+ready.delay_seconds+'-second contract delay starts when mined; execute after readiness.':'Atomic migration submitted: '+hash+'. Wait for finality, then activate this project ledger.');
   }else{setNotice(operation==='migration-plan'?'Consent plan created. Each holder can review and sign from this project in My credits.':'Migration state updated. No asset transaction was broadcast by this action.');}
   try{await refresh();}catch{/* Keep transaction hash/result even if list refresh fails. */}return output;}catch(e){setError(e instanceof Error?e.message:'Migration failed');throw e;}finally{setBusy('');}
  }}/>:null;})()}</DeploymentMigration>:null}{listing&&category==='approvals'?<><div className="provider-actions"><h2>Waiting for launch approval</h2><button className="outline-button" disabled={!!busy} onClick={()=>void run('Refreshing',()=>refresh())}>Refresh approval queue</button></div><p>Only finalized stake with a passing RPC check from the last hour can enter this queue.</p>{queues.ready.length?queues.ready.map(project=><div key={project._id}>{renderCard(project)}</div>):<div className="setup-note" role="status">No launches are ready for approval.</div>}<details className="panel section-gap"><summary>Waiting for prerequisites ({queues.waiting.length}) · no approval available</summary>{queues.waiting.map(project=><div key={project._id}>{renderCard(project)}</div>)}</details><details className="panel section-gap"><summary>On-chain verification pending ({queues.pending.length})</summary>{queues.pending.map(project=><div key={project._id}>{renderCard(project)}</div>)}</details><details className="panel section-gap"><summary>Accepted launches ({queues.accepted.length})</summary>{queues.accepted.map(project=><div key={project._id}>{renderCard(project)}</div>)}</details></>:null}</section>;
}
function ReviewCard({project,adminWallet,busy,onApprove,onVerify,onRetest}:{project:ReviewProject;adminWallet:string;busy:boolean;onApprove:(reason:string)=>void;onVerify:()=>void;onRetest:()=>void}){
 const [reason,setReason]=useState(''),[confirmed,setConfirmed]=useState(false);
 const approved=project.capacity_approval?.wallet.toLowerCase()===adminWallet.toLowerCase();
 const checked=project.progress?.rpc_checked_at??0;const fresh=checked>0&&checked<=Date.now()/1000&&checked>Date.now()/1000-3600;
 const awaiting=project.status==='awaiting-rpc';
 const deadlinePassed=!!project.rpc_deadline&&project.rpc_deadline<=Date.now()/1000;
 const needsRpc=awaiting&&project.stake_finalized&&!deadlinePassed&&(project.progress?.rpc_status!=='passed'||!fresh);
 const blocker=approvalBlocker(project,Date.now()/1000);
 return <article className="admin-review-card"><div className="provider-actions"><h2>{project.name}</h2><span className="status-badge">{projectBadge(project)}</span></div><p className="form-hint" style={{overflowWrap:'anywhere'}}>Project {project._id}<br/>Launch {project.launch_id??'Not assigned'}<br/>Provider wallet {project.wallet_address??'Not bound'}</p><dl className="resource-review"><dt>RPC check</dt><dd>{project.progress?.rpc_status??'Not tested'}{project.progress?.rpc_status==='passed'&&!fresh?' · stale; rerun Retest RPC':''}{project.progress?.rpc_checked_at?' · '+new Date(project.progress.rpc_checked_at*1000).toLocaleString():''}</dd><dt>Acceptance deadline</dt><dd>{project.rpc_deadline?new Date(project.rpc_deadline*1000).toLocaleString():'No finalized stake'}</dd><dt>Capacity decision</dt><dd>{project.capacity_approval?'Manual demo approval · not benchmarked':blocker?'Unavailable until prerequisites pass':'Pending admin review'}</dd><dt>On-chain acceptance</dt><dd>{project.status==='accepted'?'Finalized · sales closed':project.status==='verification-pending'?'Pending Ethereum finality':'Not accepted'}</dd></dl>{project.status==='verification-pending'?<div className="setup-note" role="status"><strong>Verification submitted · awaiting Ethereum finality</strong><p>No further approval is needed. Do not submit another transaction. This page refreshes status automatically every 10 seconds while idle.</p></div>:project.status==='accepted'?<div className="setup-note" role="status"><strong>On-chain verification finalized</strong><p>Launch acceptance is complete. Credit sales remain closed.</p></div>:blocker?<div className="admin-approval-blocker" role="status"><strong>Waiting for prerequisites</strong><p>{blocker} Approval is unavailable until all requirements pass.</p>{needsRpc?<><p className="form-hint">{project.rpc_retest_available?'Use Retest RPC below. Your admin wallet signs the request; the backend uses the project-bound credential without exposing it here.':'The provider has not authorized platform rechecks yet. They must save the test credential once in provider setup; the admin never needs their account or API key.'}</p></>:null}</div>:null}<div className="provider-actions"><button className="outline-button" disabled={busy||!project.stake_finalized||(deadlinePassed&&project.status!=='accepted')||!project.rpc_retest_available||!['awaiting-rpc','verification-pending','accepted'].includes(project.status)} onClick={onRetest}>Retest RPC · sign admin authorization</button><span className="form-hint">{project.rpc_retest_available?'Platform test credential authorized':'Platform test credential not authorized'}</span></div>{approved&&project.capacity_approval?<p>Approved {new Date(project.capacity_approval.at*1000).toLocaleString()} · {project.capacity_approval.reason}</p>:!blocker?<><label htmlFor={'reason-'+project._id}>Approval reason</label><textarea id={'reason-'+project._id} value={reason} maxLength={1000} onChange={e=>setReason(e.target.value)} disabled={busy}/><label className="admin-confirm"><input type="checkbox" checked={confirmed} onChange={e=>setConfirmed(e.target.checked)} disabled={busy}/>Approve this demo without a capacity benchmark. No measured throughput is certified.</label><button className="outline-button" disabled={busy||!!blocker||!confirmed||reason.trim().length<8} onClick={()=>onApprove(reason.trim())}>Sign manual capacity approval</button></>:null}{approved&&awaiting?<p className="setup-note">Capacity approval is recorded. Next: submit verifyProject on the configured chain and wait for Ethereum finality. After signing the authorization message, confirm the Sepolia transaction in your wallet. It uses test ETH for gas; credit sales remain closed.</p>:null}{approved&&awaiting?<button className="primary-button" disabled={busy||!!blocker} onClick={onVerify}>Sign verifyProject transaction</button>:null}{project.verification_tx_hash?<p><a className="text-link" target="_blank" rel="noreferrer" href={'https://sepolia.etherscan.io/tx/'+project.verification_tx_hash}>View verification transaction ↗</a></p>:null}</article>;
}
