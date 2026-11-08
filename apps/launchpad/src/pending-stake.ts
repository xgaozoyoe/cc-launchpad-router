// Retain the transaction across navigation/reloads, even before server indexing.
const memory=new Map<string,string>();
const key=(id:string)=>'acc:pending-stake:11155111:'+id;
export function pendingStake(id:string):string {
 try{return localStorage.getItem(key(id))||memory.get(id)||'';}catch{return memory.get(id)||'';}
}
export function savePendingStake(id:string,hash:string){
 memory.set(id,hash);
 try{localStorage.setItem(key(id),hash);}catch{/* In-memory protection remains available. */}
}
export function clearPendingStake(id:string){
 memory.delete(id);
 try{localStorage.removeItem(key(id));}catch{/* Storage may be unavailable. */}
}
