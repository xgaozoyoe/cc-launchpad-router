import { initialState } from './initial.ts';
import { fmt, money, precise, units } from './format.ts';
import { findModel, legacyModelId, alternativeOffers } from './models.ts';
import type { DemoAction, DemoState, Launch, PoolOperator, Receipt, Activity } from './types';

const validNumber = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n) && n >= 0;
const record = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const text = (v: unknown): v is string => typeof v === 'string' && v.length <= 200;
const family = (v: unknown) => v === 'Llama' || v === 'Qwen' || v === 'Mistral';
function isLaunch(v: unknown): v is Launch {
  return record(v) && ['id','name','supplier','precision','context','logo','color','window'].every(k=>text(v[k])) && family(v.family) && ['price','total','sold','stake'].every(k=>validNumber(v[k])) && Number(v.sold)<=Number(v.total) && typeof v.own==='boolean';
}
function isProvider(v: unknown): v is PoolOperator {return record(v)&&text(v.name)&&text(v.region)&&validNumber(v.capacity);}
function isReceipt(v: unknown): v is Receipt {return record(v)&&text(v.id)&&text(v.model)&&typeof v.outage==='boolean'&&['tokens','cost','extra','slash'].every(k=>validNumber(v[k]))&&(v.requestId===undefined||text(v.requestId));}
const activityIcons = new Set(['check','layers','wallet','shield','coin','burn','server','clock','bolt']);
function isActivity(v: unknown): v is Activity {return record(v)&&text(v.title)&&text(v.detail)&&text(v.time)&&typeof v.icon==='string'&&activityIcons.has(v.icon);}
export function parseSavedState(raw: string | null): DemoState | null {
  if (!raw) return null;
  try {
    const v: unknown=JSON.parse(raw);
    if (!record(v)||!['wallet','burned','stake','reserve','poolCapacity'].every(k=>validNumber(v[k]))||!record(v.credits)||!Object.values(v.credits).every(validNumber)||!Array.isArray(v.launches)||!v.launches.every(isLaunch)||!Array.isArray(v.poolOperators)||!v.poolOperators.every(isProvider)||!Array.isArray(v.receipts)||!v.receipts.every(isReceipt)||!Array.isArray(v.activity)||!v.activity.every(isActivity)||!(v.lastRequest===null||isReceipt(v.lastRequest))) return null;
    const launches=v.launches.map(l=>({...l,modelId:l.modelId??legacyModelId(l)}));
    if(launches.some(l=>!findModel(l.modelId)))return null;
    for(const offer of alternativeOffers)if(!launches.some(l=>l.id===offer.id))launches.push({...offer});
    if(v.prlAvailable!==undefined&&!validNumber(v.prlAvailable))return null;
    if(v.prlTransfers!==undefined&&(!Array.isArray(v.prlTransfers)||!v.prlTransfers.every(t=>record(t)&&text(t.id)&&['deposit','withdrawal'].includes(String(t.kind))&&validNumber(t.amount)&&text(t.at))))return null;
    if (new Set(launches.map(l=>l.id)).size!==launches.length||Object.keys(v.credits).some(id=>!launches.some(l=>l.id===id))) return null;
    return {prlAvailable:v.prlAvailable===undefined?10000:Number(v.prlAvailable),prlTransfers:(v.prlTransfers??[]) as DemoState['prlTransfers'],version:2,wallet:Number(v.wallet),credits:v.credits as Record<string,number>,burned:Number(v.burned),stake:Number(v.stake),reserve:Number(v.reserve),poolCapacity:Number(v.poolCapacity),launches,poolOperators:v.poolOperators,receipts:v.receipts,activity:v.activity,lastRequest:v.lastRequest,notice:null};
  } catch {return null;}
}
export function loadState(storageKey='acc-launchpad-v2',allowLegacy=true): DemoState {
  try {return parseSavedState(localStorage.getItem(storageKey))??(allowLegacy?parseSavedState(localStorage.getItem('acc-launchpad-v1')):null)??initialState();} catch {return initialState();}
}
function requireCondition(value: unknown, message: string): asserts value {if(!value) throw new Error(message);}
function commit(state: DemoState, id: string, text: string, detail: string, icon: Activity['icon']): DemoState {
  return {...state,notice:{id,text},activity:[{title:text,detail,icon,time:'Just now'},...state.activity].slice(0,12)};
}
export function demoReducer(state: DemoState, action: DemoAction): DemoState {
  try {
    if (action.type==='reset') return {...initialState(),notice:{id:action.id,text:'Demo reset. Starting balances and launches restored.'}};
    if(action.type==='prl-transfer'){
      if(state.prlTransfers.some(t=>t.id===action.id))return state;
      requireCondition(['deposit','withdrawal'].includes(action.kind),'Invalid transfer type.');
      requireCondition(Number.isSafeInteger(action.amount)&&action.amount>0&&action.amount<=1000000,'Enter a whole PRL amount between 1 and 1,000,000.');
      requireCondition(action.kind==='deposit'||action.amount<=state.prlAvailable,'Only available PRL can be withdrawn. Staked PRL remains locked.');
      const transfer={id:action.id,kind:action.kind,amount:action.amount,at:action.at};
      return commit({...state,prlAvailable:state.prlAvailable+(action.kind==='deposit'?action.amount:-action.amount),prlTransfers:[transfer,...state.prlTransfers]},action.id,action.kind==='deposit'?'Demo PRL deposited':'Demo PRL withdrawn',`${fmt(action.amount)} PRL · simulated, no on-chain transfer`,'wallet');
    }
    if (action.type==='buy') {
      const launch=state.launches.find(l=>l.id===action.launchId),q=action.quantity;
      requireCondition(launch,'This model launch is no longer available.');
      requireCondition(Number.isSafeInteger(q)&&q>=1000&&q%1000===0&&q<=launch.total-launch.sold,'Choose available credits in increments of 1,000.');
      const cost=q/1e6*launch.price;
      requireCondition(cost<=state.wallet,'Not enough demo funds. Reduce the quantity or reset the demo.');
      return commit({...state,wallet:state.wallet-cost,credits:{...state.credits,[launch.id]:(state.credits[launch.id]??0)+q},launches:state.launches.map(l=>l.id===launch.id?{...l,sold:l.sold+q}:l)},action.id,'Credits purchased',`${units(q)} ${launch.name} credits · ${money(cost)} held in escrow`,'wallet');
    }
    if (action.type==='redeem') {
      if (state.receipts.some(r=>r.requestId===action.id)) return state;
      const launch=state.launches.find(l=>l.id===action.launchId),tokens=action.tokens;
      requireCondition(launch,'This model launch is no longer available.');
      requireCondition(Number.isSafeInteger(tokens)&&tokens>0,'Invalid usage quantity.');
      requireCondition((state.credits[launch.id]??0)>=tokens,'Insufficient credits for this model. Your balance remains unspent.');
      const cost=tokens/1e6*launch.price,extra=action.outage?cost*.5:0;
      requireCondition(!action.outage||(state.poolCapacity>0&&state.reserve>=extra),'Pool protection unavailable. Credits remain unspent; refund protection applies.');
      const slash=action.outage?Math.min(extra/2,launch.stake):0;
      const receipt:Receipt={requestId:action.id,tokens,model:launch.name,id:launch.id,outage:action.outage,cost,extra,slash};
      const next={...state,credits:{...state.credits,[launch.id]:state.credits[launch.id]-tokens},burned:state.burned+tokens,reserve:state.reserve-extra+slash*2,launches:state.launches.map(l=>l.id===launch.id?{...l,stake:l.stake-slash}:l),lastRequest:receipt,receipts:[receipt,...state.receipts]};
      return commit(next,action.id,action.outage?'Public pool completed request':'Inference completed',`${fmt(tokens)} credits burned${action.outage?' · '+precise(slash)+' PRL penalty':''}`,action.outage?'layers':'burn');
    }
    if (action.type==='launch') {
      if(state.launches.some(l=>l.id===action.id))return state;
      const model=findModel(action.modelId);
      requireCondition(model,'Choose a supported model from the catalog.');
      requireCondition(action.verified,'Run the capacity benchmark first.');
      requireCondition(action.name.trim().length>0&&action.name.length<=60,'Choose a launch name with 1–60 characters.');
      requireCondition(validNumber(action.price)&&action.price>=.1&&action.price<=100,'Set a price between $0.10 and $100 per million credits.');
      requireCondition(Number.isSafeInteger(action.total)&&action.total>=1e6&&action.total<=1e8&&action.total%1e6===0,'Choose a supply between 1M and 100M credits.');
      requireCondition(Number.isSafeInteger(action.stake)&&action.stake>=1000&&action.stake<=100000&&action.stake%100===0,'Stake between 1,000 and 100,000 PRL in increments of 100.');
      requireCondition(action.total<=action.stake*10000,'Increase your PRL stake or reduce supply to satisfy the demo collateral limit.');
      requireCondition(action.stake<=state.prlAvailable,'Insufficient available PRL. Deposit demo PRL in your PRL wallet first.');
      const launch:Launch={id:action.id,resourceId:action.resourceId,modelId:model.id,name:action.name.trim(),supplier:'Your compute workspace',family:model.family,price:action.price,total:action.total,sold:0,stake:action.stake,precision:model.precision,context:model.context,logo:model.family[0],color:model.family==='Qwen'?'blue':model.family==='Mistral'?'purple':'',window:action.window,own:true};
      return commit({...state,prlAvailable:state.prlAvailable-action.stake,launches:[...state.launches,launch]},action.id,'New credit launch created',`${launch.name} · ${units(launch.total)} credits · ${fmt(launch.stake)} PRL locked`,'coin');
    }
    requireCondition(action.name.trim().length>0&&action.name.length<=50,'Choose a provider name with 1–50 characters.');
    requireCondition(validNumber(action.capacity)&&action.capacity>=1&&action.capacity<=100&&Number.isInteger(action.capacity),'Reserve between 1K and 100K tokens/sec.');
    return commit({...state,poolCapacity:state.poolCapacity+action.capacity,poolOperators:[...state.poolOperators,{name:action.name.trim(),region:action.region,capacity:action.capacity}]},action.id,'Pool provider enrolled',`${action.name.trim()} · ${units(action.capacity*1000)} tokens/sec reserved`,'layers');
  } catch(e) {return {...state,notice:{id:action.id,text:e instanceof Error?e.message:'The action could not complete.',error:true}};}
}
