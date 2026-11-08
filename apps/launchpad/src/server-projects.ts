export type ServerProject={_id:string;name:string;model_id:string;status:string;wallet_address?:string;stake_raw?:string;onchain_key?:string;stake_tx_hash?:string;launch_id?:string;provider_id?:string;staked_at?:number;rpc_deadline?:number;registration_error?:string;stake_finalized?:boolean;capacity_approval?:{wallet:string;at:number;reason:string;type:string};progress?:{bundle_issued:boolean;last_proxy_at?:number;rpc_status?:string;rpc_checked_at?:number}};
export async function serverApi<T>(path:string,options?:RequestInit):Promise<T>{
 const response=await fetch('/api/central/'+path,{...options,credentials:'same-origin',cache:'no-store'});
 const data=await response.json() as T&{error?:string};
 if(!response.ok)throw new Error(data.error??'The backend request failed.');return data;
}
export function serverPost<T>(path:string,body:unknown){return serverApi<T>(path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});}
