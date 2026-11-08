import {useEffect,useReducer,useState} from 'react';
import type {DemoAction,View} from '../types';
import {demoReducer,loadState} from '../state';
import {Overview,Credits,Pool} from './Pages';
import {ModelCatalog,BrowseCredits} from './ModelCatalog';
import {PrlWallet} from './PrlWallet';
import {BuyDialog,GuideDialog,PoolDialog} from './Dialogs';
import {ProjectLaunch} from './ProjectLaunch';
type DemoDialog={type:'buy';id:string}|{type:'launch';modelId?:string}|{type:'pool'|'guide'}|null;
const tabs:ReadonlyArray<{view:View;label:string}>=[{view:'launchpad',label:'Overview'},{view:'browse',label:'Buy simulated credits'},{view:'credits',label:'Demo credits'},{view:'prl',label:'Demo PRL wallet'},{view:'models',label:'Illustrative models'},{view:'pool',label:'Demo public pool'}];
export function DemoWorkspace({accountId,onExit}:{accountId:string;onExit:()=>void}){
 const key='acc-launchpad-v2:'+accountId;
 const [state,dispatch]=useReducer(demoReducer,undefined,()=>loadState(key,false));
 const [view,setView]=useState<View>('launchpad'),[dialog,setDialog]=useState<DemoDialog>(null);
 useEffect(()=>{try{localStorage.setItem(key,JSON.stringify({...state,notice:null}));}catch{}},[key,state]);
 function perform(action:DemoAction){const result=demoReducer(state,action);dispatch(action);return !result.notice?.error;}
 const navigate=(next:View)=>setView(tabs.some(tab=>tab.view===next)?next:'launchpad');
 const props={state,perform,navigate,openLaunch:()=>setDialog({type:'launch'}),openGuide:()=>setDialog({type:'guide'}),openBuy:(id:string)=>setDialog({type:'buy',id}),openPool:()=>setDialog({type:'pool'})};
 return <section className="demo-workspace"><div className="demo-workspace-banner"><div><strong>DEMO · SIMULATED DATA</strong><p>All providers, prices, balances, purchases, inference and pool activity in this workspace are illustrative. Demo actions never create backend projects or send blockchain transactions.</p></div><button className="outline-button" onClick={onExit}>Return to real Launchpad</button></div><div className="filterbar demo-tabs" aria-label="Demo sections">{tabs.map(tab=><button key={tab.view} aria-pressed={view===tab.view} className={view===tab.view?'active':''} onClick={()=>setView(tab.view)}>{tab.label}</button>)}<button onClick={()=>dispatch({type:'reset',id:crypto.randomUUID()})}>Reset demo</button></div>{state.notice?<p className={state.notice.error?'error':'setup-note'} role="status">{state.notice.text}</p>:null}{view==='launchpad'?<Overview {...props}/>:view==='browse'?<BrowseCredits state={state} onBuy={props.openBuy} onModels={()=>setView('models')}/>:view==='models'?<ModelCatalog state={state} onBrowse={()=>setView('browse')} onLaunch={modelId=>setDialog({type:'launch',modelId})}/>:view==='credits'?<Credits {...props}/>:view==='prl'?<PrlWallet state={state} perform={perform} onLaunch={props.openLaunch}/>:<Pool {...props}/>}{dialog?.type==='buy'?<BuyDialog state={state} perform={perform} launchId={dialog.id} onClose={()=>setDialog(null)}/>:dialog?.type==='launch'?<ProjectLaunch state={state} perform={perform} initialModelId={dialog.modelId} onClose={()=>setDialog(null)}/>:dialog?.type==='pool'?<PoolDialog state={state} perform={perform} onClose={()=>setDialog(null)}/>:dialog?.type==='guide'?<GuideDialog onClose={()=>setDialog(null)}/>:null}</section>;
}
