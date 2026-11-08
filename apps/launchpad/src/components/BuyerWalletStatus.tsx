import {useEffect,useState} from 'react';
type Provider={request:(args:{method:string})=>Promise<unknown>;on?:(event:string,listener:()=>void)=>void;removeListener?:(event:string,listener:()=>void)=>void};
export function BuyerWalletStatus({wallet,chainId,busy,onConnect}:{wallet?:string|null;chainId?:number;busy:boolean;onConnect:()=>void}){
 const [status,setStatus]=useState('Checking wallet…');
 useEffect(()=>{let active=true,version=0;const p=(window as unknown as {ethereum?:Provider}).ethereum;
 const check=async()=>{const current=++version;let next='Connect wallet';try{if(!wallet)next='Connect wallet';else if(!p)next='Open with a wallet extension';else{const [accounts,chain]=await Promise.all([p.request({method:'eth_accounts'}),p.request({method:'eth_chainId'})]);next=!Array.isArray(accounts)||!accounts.length?'Connect wallet':String(accounts[0]).toLowerCase()!==wallet.toLowerCase()?'Select your buyer wallet':!chainId||BigInt(String(chain))!==BigInt(chainId)?'Switch network':'Connected';}}catch{next='Reconnect wallet';}if(active&&current===version)setStatus(next);};
 const listener=()=>{void check();};listener();p?.on?.('accountsChanged',listener);p?.on?.('chainChanged',listener);p?.on?.('disconnect',listener);window.addEventListener('focus',listener);const timer=setInterval(listener,10000);
 return()=>{active=false;clearInterval(timer);p?.removeListener?.('accountsChanged',listener);p?.removeListener?.('chainChanged',listener);p?.removeListener?.('disconnect',listener);window.removeEventListener('focus',listener);};},[wallet,chainId]);
 return <div className="setup-note"><strong role="status">{status}{status==='Connected'?' · '+(chainId===11155111?'Sepolia':'Chain '+chainId):''}</strong>{wallet?<p className="real-id">{wallet}</p>:<p>Connect a wallet once to access your credits.</p>}{status!=='Connected'&&status!=='Checking wallet…'?<button className="outline-button" disabled={busy||!chainId} onClick={onConnect}>{status}</button>:null}</div>;
}
