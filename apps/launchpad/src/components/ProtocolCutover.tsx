import {useEffect,useRef,useState} from 'react';
import {serverPost} from '../server-projects';
type Audit={phase:string;can_activate:boolean;legacy_staking_contract?:string;blockers:string[];note?:string;candidates?:{id:string;version:string;contracts:Record<string,string>;validation:{code_and_bindings:string;error?:string;finalized_block?:string;finalized_block_hash?:string}}[]};
/** Read-only migration readiness; intentionally has no activation or transaction action. */
export function ProtocolCutover({token}:{token:string}){
 const [report,setReport]=useState<Audit|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const generation=useRef(0),running=useRef(false);
 useEffect(()=>{generation.current++;running.current=false;setReport(null);setError('');setBusy(false);return()=>{generation.current++;};},[token]);
 async function check(){
  if(!token||running.current)return;running.current=true;const active=generation.current;setBusy(true);setError('');
  try{const data=await serverPost<Audit>('admin/protocol',{token});if(generation.current===active)setReport(data);}
  catch(e){if(generation.current===active)setError(e instanceof Error?e.message:'Could not check contract readiness');}
  finally{if(generation.current===active){running.current=false;setBusy(false);}}
 }
 return <section className="panel section-gap" aria-labelledby="protocol-cutover-title"><h2 id="protocol-cutover-title">Target contract audit</h2>
 <p>Legacy contracts continue serving existing balances. This check validates staged contracts; it does not activate them, move funds or request a wallet signature.</p>
 <button className="outline-button" disabled={busy||!token} onClick={()=>void check()}>{busy?'Checking finalized contracts…':'Check migration readiness'}</button>
 {!token?<p className="form-hint">Connect and sign with the admin wallet above to inspect migration readiness.</p>:null}
 {error?<p className="error" role="alert">{error}</p>:null}
 {report?<div aria-live="polite"><p><strong>{report.phase==='staged'?'Candidate staged · activation unavailable':report.phase==='preparing'?'Preparing new contracts · legacy remains active':'Migration check requires attention'}</strong></p>
 {report.legacy_staking_contract?<p className="real-id">Active legacy stake: {report.legacy_staking_contract}</p>:null}
 {report.candidates?.map(candidate=><div key={candidate.id}><h3>{candidate.id} · {candidate.version}</h3><p>{candidate.validation.code_and_bindings==='passed'?'Finalized bytecode and peer bindings verified.':'Contract verification failed: '+(candidate.validation.error??'Unknown failure')}</p>{candidate.validation.finalized_block_hash?<p className="real-id">Checkpoint: {candidate.validation.finalized_block_hash}</p>:null}<details><summary>Candidate addresses</summary><dl className="resource-review">{Object.entries(candidate.contracts).map(([name,address])=><div key={name}><dt>{name}</dt><dd className="real-id">{address}</dd></div>)}</dl></details></div>)}
 <h3>Required before switching</h3><ul>{report.blockers.map(blocker=><li key={blocker}>{blocker}</li>)}</ul>
 <p className="form-hint">{report.note??'Existing funds and project identities are unchanged.'}</p></div>:null}
 </section>;
}
