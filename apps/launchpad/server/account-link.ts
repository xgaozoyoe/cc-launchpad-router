import {createHash,createHmac,randomBytes} from 'node:crypto';
export async function linkLegacyAccount(env:{CENTRAL_API_ORIGIN?:string;CENTRAL_HMAC_KEY?:string},canonical:string,legacy:string){
 if(!env.CENTRAL_API_ORIGIN)return; // Separate frontend-only concept deployments have no server records.
 const base=new URL(env.CENTRAL_API_ORIGIN);
 if(base.username||base.password||base.pathname!=='/'||base.search||base.hash||!(base.protocol==='https:'||(base.protocol==='http:'&&['localhost','127.0.0.1','central'].includes(base.hostname))))throw new Error('Invalid backend origin');
 if(!/^[a-fA-F0-9]{64}$/.test(env.CENTRAL_HMAC_KEY??''))throw new Error('Account migration signing is unavailable');
 const path='/api/v1/account/link-legacy',body=JSON.stringify({legacy_account_id:legacy}),stamp=Math.floor(Date.now()/1000).toString(),nonce=randomBytes(16).toString('hex');
 const signature=createHmac('sha256',Buffer.from(env.CENTRAL_HMAC_KEY!,'hex')).update(['POST',path,canonical,stamp,nonce,createHash('sha256').update(body).digest('hex')].join('\n')).digest('hex');
 const response=await fetch(new URL(path,base),{method:'POST',headers:{'Content-Type':'application/json','X-CC-User':canonical,'X-CC-Timestamp':stamp,'X-CC-Nonce':nonce,'X-CC-Signature':signature},body,redirect:'error',signal:AbortSignal.timeout(10000)});
 if(!response.ok)throw new Error('Account migration failed; existing session preserved');
}
