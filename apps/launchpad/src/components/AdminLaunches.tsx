import {useEffect,useRef,useState} from 'react';
import {ProtocolCutover} from './ProtocolCutover';
import {serverPost} from '../server-projects';
import type {ServerProject} from '../server-projects';
import {ensureWalletChain} from '../wallet-network';
import {projectBadge} from '../launch-dashboard';
type Wallet={request:(args:{method:string;params?:unknown[]})=>Promise<unknown>;on?:(event:string,fn:()=>void)=>void;removeListener?:(event:string,fn:()=>void)=>void};
type ReviewProject=ServerProject&{capacity_approval?:{wallet:string;at:number;reason:string;type:string};rpc_retest_available?:boolean;verification_tx_hash?:string};
type Listing={projects:ReviewProject[];admin_address:string;chain_id:number;contract:string};
const provider=()=>{const p=(window as unknown as {ethereum?:Wallet}).ethereum;if(!p)throw new Error('Open this page in a browser with an Ethereum wallet extension.');return p;};
const messageHex=(message:string)=>'0x'+Array.from(new TextEncoder().encode(message),b=>b.toString(16).padStart(2,'0')).join('');
export function AdminLaunches(){
 const [wallet,setWallet]=useState(''),[token,setToken]=useState(''),[listing,setListing]=useState<Listing|null>(null),[busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const epoch=useRef(0),switchingNetwork=useRef(false);
 const pendingFinality=listing?.projects.some(p=>p.status==='verification-pending')??false;
 useEffect(()=>{
  if(!token||!pendingFinality||busy)return;
  const generation=epoch.current;let active=true,running=false;
  const timer=setInterval(()=>{if(running)return;running=true;void serverPost<Listing>('admin/query',{token}).then(data=>{if(active&&epoch.current===generation)setListing(data);}).catch(e=>{if(active){setError(e instanceof Error?e.message:'Could not refresh verification progress');clearInterval(timer);}}).finally(()=>{running=false;});},10000);
  return()=>{active=false;clearInterval(timer);};
 },[token,pendingFinality,busy]);
 useEffect(()=>{const p=(window as unknown as {ethereum?:Wallet}).ethereum;const clear=()=>{epoch.current++;setWallet('');setToken('');setListing(null);setNotice('Wallet changed. Connect and sign again.');};const chainChanged=()=>{if(!switchingNetwork.current)clear();};p?.on?.('accountsChanged',clear);p?.on?.('chainChanged',chainChanged);return()=>{epoch.current++;p?.removeListener?.('accountsChanged',clear);p?.removeListener?.('chainChanged',chainChanged);};},[]);
 async function signed<T>(intent:string,payload:unknown,address:string){
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
  const signature=await provider().request({method:'personal_sign',params:[messageHex(challenge.message),address]});
  if(epoch.current!==started)throw new Error('Wallet changed. Reconnect before continuing.');
  const result=await serverPost<T>('admin/'+(intent==='login'?'authenticate':intent),{challenge_id:challenge.challenge_id,signature});
  if(epoch.current!==started)throw new Error('Wallet changed. Reconnect before continuing.');return result;
 }
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
 return <section className="admin-page"><div className="page-heading"><div><span className="eyebrow">PLATFORM ADMINISTRATION</span><h1>Launch review</h1><p>Approve a demo capacity exception, then sign on-chain acceptance with the authorized wallet.</p></div></div><div className="setup-note"><strong>Wallet authorization required</strong><p>The server verifies a fresh signature for every approval. Google sign-in alone grants no admin authority. Connecting requests the configured network automatically; confirm the switch in your wallet. Sales stay closed after acceptance.</p><button className="primary-button" disabled={!!busy} onClick={()=>void run('Connecting wallet',connect)}>{token?'Reconnect admin wallet':'Connect admin wallet & sign'}</button>{wallet?<p style={{overflowWrap:'anywhere'}}>Connected: {wallet}</p>:null}</div>{error?<p className="error" role="alert">{error}</p>:null}{notice?<p role="status" style={{overflowWrap:'anywhere'}}>{notice}</p>:null}{busy?<p role="status">{busy}…</p>:null}<ProtocolCutover token={token}/>{listing?<><div className="provider-actions"><p>Chain {listing.chain_id} · Contract {listing.contract}</p><button className="outline-button" disabled={!!busy} onClick={()=>void run('Refreshing',()=>refresh())}>Refresh review status</button></div><p className="form-hint">Acceptance is finalized by the chain monitor. Refresh after Ethereum finality. A passing RPC check within the last hour is required before approval.</p>{listing.projects.map(project=><ReviewCard key={project._id} project={project} adminWallet={listing.admin_address} onRetest={()=>void run('Running signed RPC recheck',()=>retest(project))} busy={!!busy} onApprove={reason=>void run('Signing capacity approval',()=>approve(project,reason))} onVerify={()=>void run('Signing on-chain verification',()=>verify(project))}/>)}</>:null}</section>;
}
function ReviewCard({project,adminWallet,busy,onApprove,onVerify,onRetest}:{project:ReviewProject;adminWallet:string;busy:boolean;onApprove:(reason:string)=>void;onVerify:()=>void;onRetest:()=>void}){
 const [reason,setReason]=useState(''),[confirmed,setConfirmed]=useState(false);
 const approved=project.capacity_approval?.wallet.toLowerCase()===adminWallet.toLowerCase();
 const checked=project.progress?.rpc_checked_at??0;const fresh=checked>0&&checked<=Date.now()/1000&&checked>Date.now()/1000-3600;
 const awaiting=project.status==='awaiting-rpc';
 const deadlinePassed=!!project.rpc_deadline&&project.rpc_deadline<=Date.now()/1000;
 const needsRpc=awaiting&&project.stake_finalized&&!deadlinePassed&&(project.progress?.rpc_status!=='passed'||!fresh);
 const blocker=!awaiting?'This project is not awaiting approval.':!project.stake_finalized?'Wait for the stake to be finalized.':deadlinePassed?'The on-chain acceptance deadline has passed.':project.progress?.rpc_status!=='passed'?'A passing Test RPC result is required before signing approval.':!fresh?'The RPC result is over one hour old or its timestamp is invalid. Rerun Test RPC before signing approval.':null;
 return <article className="admin-review-card"><div className="provider-actions"><h2>{project.name}</h2><span className="status-badge">{projectBadge(project)}</span></div><p className="form-hint" style={{overflowWrap:'anywhere'}}>Project {project._id}<br/>Launch {project.launch_id??'Not assigned'}<br/>Provider wallet {project.wallet_address??'Not bound'}</p><dl className="resource-review"><dt>RPC check</dt><dd>{project.progress?.rpc_status??'Not tested'}{project.progress?.rpc_status==='passed'&&!fresh?' · stale; rerun Retest RPC':''}{project.progress?.rpc_checked_at?' · '+new Date(project.progress.rpc_checked_at*1000).toLocaleString():''}</dd><dt>Acceptance deadline</dt><dd>{project.rpc_deadline?new Date(project.rpc_deadline*1000).toLocaleString():'No finalized stake'}</dd><dt>Capacity decision</dt><dd>{project.capacity_approval?'Manual demo approval · not benchmarked':'Pending'}</dd><dt>On-chain acceptance</dt><dd>{project.status==='accepted'?'Finalized · sales closed':project.status==='verification-pending'?'Pending Ethereum finality':'Not accepted'}</dd></dl>{project.status==='verification-pending'?<div className="setup-note" role="status"><strong>Verification submitted · awaiting Ethereum finality</strong><p>No further approval is needed. Do not submit another transaction. This page refreshes status automatically every 10 seconds while idle.</p></div>:project.status==='accepted'?<div className="setup-note" role="status"><strong>On-chain verification finalized</strong><p>Launch acceptance is complete. Credit sales remain closed.</p></div>:blocker?<div className="admin-approval-blocker" role="status"><strong>Approval blocked</strong><p>{blocker} You can still prepare the approval reason below.</p>{needsRpc?<><p className="form-hint">{project.rpc_retest_available?'Use Retest RPC below. Your admin wallet signs the request; the backend uses the project-bound credential without exposing it here.':'The provider has not authorized platform rechecks yet. They must save the test credential once in provider setup; the admin never needs their account or API key.'}</p></>:null}</div>:null}<div className="provider-actions"><button className="outline-button" disabled={busy||!project.stake_finalized||(deadlinePassed&&project.status!=='accepted')||!project.rpc_retest_available||!['awaiting-rpc','verification-pending','accepted'].includes(project.status)} onClick={onRetest}>Retest RPC · sign admin authorization</button><span className="form-hint">{project.rpc_retest_available?'Platform test credential authorized':'Platform test credential not authorized'}</span></div>{approved&&project.capacity_approval?<p>Approved {new Date(project.capacity_approval.at*1000).toLocaleString()} · {project.capacity_approval.reason}</p>:<><label htmlFor={'reason-'+project._id}>Approval reason</label><textarea id={'reason-'+project._id} value={reason} maxLength={1000} onChange={e=>setReason(e.target.value)} disabled={busy}/><label className="admin-confirm"><input type="checkbox" checked={confirmed} onChange={e=>setConfirmed(e.target.checked)} disabled={busy}/>Approve this demo without a capacity benchmark. No measured throughput is certified.</label><button className="outline-button" disabled={busy||!!blocker||!confirmed||reason.trim().length<8} onClick={()=>onApprove(reason.trim())}>Sign manual capacity approval</button></>}{approved&&awaiting?<p className="setup-note">Capacity approval is recorded. Next: submit verifyProject on the configured chain and wait for Ethereum finality. After signing the authorization message, confirm the Sepolia transaction in your wallet. It uses test ETH for gas; credit sales remain closed.</p>:null}{approved&&awaiting?<button className="primary-button" disabled={busy||!!blocker} onClick={onVerify}>Sign verifyProject transaction</button>:null}{project.verification_tx_hash?<p><a className="text-link" target="_blank" rel="noreferrer" href={'https://sepolia.etherscan.io/tx/'+project.verification_tx_hash}>View verification transaction ↗</a></p>:null}</article>;
}
