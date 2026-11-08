export type AdminSession={wallet:string;token:string;chainId:number;contract:string;expiresAt:number};
type Store=Pick<Storage,'getItem'|'setItem'|'removeItem'>;
type Wallet={request:(args:{method:string;params?:unknown[]})=>Promise<unknown>};
type Identity={admin_address:string;chain_id:number;contract:string};
export const adminSessionKey=(account:string)=>'acc-admin-session:v1:'+encodeURIComponent(account);
export function forgetAdminSession(store:Store,key:string){try{store.removeItem(key);}catch{/* Storage can be unavailable. */}}
export function saveAdminSession(store:Store,key:string,session:AdminSession){try{store.setItem(key,JSON.stringify(session));}catch{/* Current in-memory session still works. */}}
export function readAdminSession(store:Store,key:string,now:number):AdminSession|null{
 try{const s=JSON.parse(store.getItem(key)??'null') as AdminSession|null;
  if(!s||!/^0x[0-9a-f]{40}$/i.test(s.wallet)||!/^0x[0-9a-f]{40}$/i.test(s.contract)||typeof s.token!=='string'||!s.token||s.token.length>512||!Number.isSafeInteger(s.chainId)||s.chainId<=0||!Number.isFinite(s.expiresAt)||s.expiresAt<=now){forgetAdminSession(store,key);return null;}
  return s;
 }catch{forgetAdminSession(store,key);return null;}
}
export async function restoreAdminSession<T extends Identity>(store:Store,key:string,wallet:Wallet,query:(token:string)=>Promise<T>,now=Date.now()):Promise<{wallet:string;session?:AdminSession;listing?:T}>{
 const saved=readAdminSession(store,key,now);
 const accounts=await wallet.request({method:'eth_accounts'});
 const address=Array.isArray(accounts)&&typeof accounts[0]==='string'&&/^0x[0-9a-f]{40}$/i.test(accounts[0])?accounts[0].toLowerCase():'';
 if(!address){forgetAdminSession(store,key);return {wallet:''};}
 if(!saved)return {wallet:address};
 const chain=await wallet.request({method:'eth_chainId'});
 if(address!==saved.wallet.toLowerCase()||typeof chain!=='string'||!/^0x[0-9a-f]+$/i.test(chain)||BigInt(chain)!==BigInt(saved.chainId)){forgetAdminSession(store,key);return {wallet:address};}
 let listing:T;
 try{listing=await query(saved.token);}catch(error){const status=(error as {status?:number}).status;if(status===401||status===403){forgetAdminSession(store,key);return {wallet:address};}throw error;}
 if(listing.admin_address.toLowerCase()!==address||listing.chain_id!==saved.chainId||listing.contract.toLowerCase()!==saved.contract.toLowerCase()){forgetAdminSession(store,key);return {wallet:address};}
 return {wallet:address,session:saved,listing};
}

const unavailableStore:Store={getItem:()=>null,setItem:()=>{},removeItem:()=>{}};
export function adminSessionStorage():Store{try{return window.sessionStorage;}catch{return unavailableStore;}}
