import { Fragment, useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import { Icon } from './Icon';
import type { IconName } from './Icon';
import type { Activity, Launch, Receipt } from '../types';
import { modelName } from '../models';
import { fmt, money, precise, units } from '../format';

export function Heading({title,description,action='Launch credits',onAction}:{title:string;description:string;action?:string;onAction:()=>void}) {
  return <div className="page-heading"><div><h1>{title}</h1><p>{description}</p></div><button className="primary-button" onClick={onAction}><Icon name={action==='Launch credits'?'plus':'arrow'}/>{action}</button></div>;
}
export function ModelLogo({launch}:{launch:Launch}) {return <span className={'model-logo '+launch.color}>{launch.logo}</span>;}
export function Flow({steps}:{steps:ReadonlyArray<readonly [IconName,string]>}) {
  return <div className="flow-row">{steps.map(([name,label],i)=><Fragment key={label}>{i>0?<span className="flow-arrow" aria-hidden="true">→</span>:null}<div className="flow-step"><div className="flow-step-icon"><Icon name={name}/></div>{label}</div></Fragment>)}</div>;
}
export function Stat({label,value,detail,icon}:{label:string;value:ReactNode;detail:ReactNode;icon:IconName}) {
  return <div className="stat"><div className="stat-label"><Icon name={icon}/>{label}</div><div className="stat-value">{value}</div><div className="stat-detail">{detail}</div></div>;
}
export function Metric({label,value,detail}:{label:string;value:ReactNode;detail:string}) {return <div className="metric-box"><p>{label}</p><strong>{value}</strong><small>{detail}</small></div>;}
export function ActivityList({items}:{items:Activity[]}) {return <div className="activity-list">{items.map((a,i)=><div className="activity-item" key={`${a.title}-${i}`}><span className="activity-symbol"><Icon name={a.icon}/></span><div><strong>{a.title}</strong><small>{a.detail}</small></div><span className="activity-time">{a.time}</span></div>)}</div>;}
export function LaunchCard({launch:l,onBuy}:{launch:Launch;onBuy:(id:string)=>void}) {
  const remaining=l.total-l.sold,pct=Math.round(l.sold/l.total*100);
  return <article className="launch-card"><div className="card-top"><ModelLogo launch={l}/><span className="status-tag">{remaining>0?'Live launch':'Fully allocated'}</span></div><h3>{l.name}</h3>{l.name!==modelName(l)?<p className="offer-model">{modelName(l)}</p>:null}<p className="supplier-name"><Icon name="shield"/>{l.supplier}</p><div className="model-tags"><span>{l.family}</span><span>{l.precision}</span><span>{l.context}</span></div><div className="price">{money(l.price)}<small>/ 1M output credits</small></div><div className="capacity"><span>{units(remaining)} credits available</span><strong>{pct}% allocated</strong></div><div className="progress" role="progressbar" aria-label={`${l.name} credits allocated`} aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}><span style={{width:`${pct}%`}}/></div><div className="stake-line"><Icon name="shield"/>{fmt(l.stake)} PRL staked · Capacity verified</div><div className="card-bottom"><span>{l.window} delivery window</span><button className="outline-button" disabled={remaining<=0} onClick={()=>onBuy(l.id)}>Buy credits <Icon name="arrow"/></button></div></article>;
}
export function ReceiptView({receipt:r}:{receipt:Receipt}) {
  const steps=[['Credits reserved',`${fmt(r.tokens)} credits · ${r.model}`],[r.outage?'Public pool delivered':'Supplier delivered',r.outage?'Supplier outage detected · automatic reroute':'Output accepted by the demo meter'],['Credits burned',`${fmt(r.tokens)} consumed · no duplicate charge`],[r.outage?'Stake reduced after demo review':'Earned payment released',r.outage?`${precise(r.slash)} PRL recovered toward $${precise(r.extra)} replacement shortfall`:`${money(r.cost)} released from buyer escrow`]];
  return <>{steps.map(([title,detail],i)=><div className="receipt-step" key={title}><span className="num">{i+1}</span><div>{title}<small>{detail}</small></div><Icon name="check"/></div>)}<p className="form-hint" style={{marginTop:15}}>{r.outage?'In production, independent evidence and an appeal process precede a final penalty.':'Unused reserved credits stay available. Only accepted usage consumes the balance.'}</p></>;
}
export function Result({receipt:r}:{receipt:Receipt}) {
  return <div className="request-result"><h3>Demo response</h3><p>Computing credits let a buyer reserve a defined amount of future inference for a specific model. The supplier commits the capacity and stakes PRL to back delivery. As the buyer uses the service, the launchpad retires the consumed credits. If the supplier fails, a compatible public-pool provider can complete the request.</p><div className="result-meta"><span>{fmt(r.tokens)} simulated output credits burned</span><span className={r.outage?'warn':''}>{r.outage?'Public-pool fallback':'Primary supplier'}</span></div></div>;
}
export function Modal({title,children,onClose}:{title:string;children:ReactNode;onClose:()=>void}) {
  const ref=useRef<HTMLDialogElement>(null);
  useEffect(()=>{const dialog=ref.current;if(dialog&&!dialog.open)dialog.showModal();return ()=>{if(dialog?.open)dialog.close();};},[]);
  return <dialog ref={ref} aria-labelledby="modal-title" onCancel={onClose} onClick={e=>{if(e.target!==e.currentTarget)return;const r=e.currentTarget.getBoundingClientRect();if(e.clientX<r.left||e.clientX>r.right||e.clientY<r.top||e.clientY>r.bottom)onClose();}}><div className="modal-top"><h2 id="modal-title">{title}</h2><button className="icon-button" onClick={onClose} aria-label="Close dialog"><Icon name="close"/></button></div><div>{children}</div></dialog>;
}
