import { useEffect, useState } from 'react';
import { App } from './App';
import { Icon } from './components/Icon';
import { readSession, signInGoogle, signOut } from './auth-client';
import type { Account } from './auth-client';
import './auth.css';
import { WhitepaperLink } from './components/WhitepaperLink';

function GoogleMark(){return <svg viewBox="0 0 24 24" aria-hidden="true"><path fill="#4285F4" stroke="none" d="M21.6 12.2c0-.7-.1-1.4-.2-2.1H12v4h5.4a4.6 4.6 0 0 1-2 3v2.5h3.3c1.9-1.8 2.9-4.3 2.9-7.4Z"/><path fill="#34A853" stroke="none" d="M12 22c2.7 0 5-1 6.7-2.4l-3.3-2.5c-.9.6-2 1-3.4 1-2.6 0-4.9-1.8-5.7-4.1H2.9v2.6A10 10 0 0 0 12 22Z"/><path fill="#FBBC05" stroke="none" d="M6.3 14a6 6 0 0 1 0-4V7.4H2.9a10 10 0 0 0 0 9.2L6.3 14Z"/><path fill="#EA4335" stroke="none" d="M12 5.9c1.5 0 2.8.5 3.8 1.5l2.9-2.8A9.5 9.5 0 0 0 12 2a10 10 0 0 0-9.1 5.4L6.3 10A6 6 0 0 1 12 5.9Z"/></svg>;}
export function AuthGate(){
  const [account,setAccount]=useState<Account|null>(null),[loading,setLoading]=useState(true),[configured,setConfigured]=useState(false),[pending,setPending]=useState(false),[error,setError]=useState('');
  useEffect(()=>{
    let active=true;
    async function load(){
      try{
        const response=await fetch('/api/auth/status',{cache:'no-store'});
        if(!response.ok)throw new Error('Sign-in is temporarily unavailable.');
        const status:unknown=await response.json();
        const ready=Boolean(status&&typeof status==='object'&&'configured' in status&&status.configured===true);
        if(!active)return;setConfigured(ready);
        if(ready){const user=await readSession();if(active)setAccount(user);}
        if(active&&new URLSearchParams(location.search).has('error'))setError('This sign-in could not be completed. Please check that your Google account has access.');
      }catch(e){if(active)setError(e instanceof Error?e.message:'Please try again.');}
      finally{if(active)setLoading(false);}
    }
    void load();return ()=>{active=false;};
  },[]);
  async function login(){setPending(true);setError('');try{await signInGoogle();}catch(e){setError(e instanceof Error?e.message:'Sign-in failed.');setPending(false);}}
  async function logout(){setPending(true);setError('');try{await signOut();setAccount(null);}catch(e){setError(e instanceof Error?e.message:'Sign-out failed.');}finally{setPending(false);}}
  if(account)return <><App key={account.id} account={account} onSignOut={logout} onReauth={account.id==='local-test-account'?undefined:login} signingOut={pending}/>{error?<div className="auth-account-error" role="alert">{error}</div>:null}</>;
  return <main className="auth-shell"><section className="auth-story"><a className="brand auth-brand" href="/" aria-label="AI Computing Credit Launchpad"><span className="brand-mark">a<span>c</span>c<span className="brand-dot"/></span><span className="brand-caption">AI COMPUTING CREDIT LAUNCHPAD</span></a><div><div className="eyebrow">AI COMPUTING CREDIT LAUNCHPAD</div><h1>Launch credits now.<br/><span>Deliver compute later.</span></h1><p>Explore real provider projects and their verification progress.<br/>Buy test credits and try real model inference.</p><WhitepaperLink/><div className="auth-proof"><Icon name="shield"/><span>On-chain test tPRL staking</span></div><div className="auth-proof"><Icon name="layers"/><span>Project-bound FRPC and RPC verification</span></div></div><small>Ethereum Sepolia testnet · Test credit purchases & real inference</small></section><section className="auth-entry"><div className="auth-card"><span className="auth-kicker">WELCOME TO THE LAUNCHPAD</span><h2>Your compute journey<br/>starts here.</h2><p>Sign in to explore real launches, manage provider setup, and track on-chain verification.</p><button className="google-button" onClick={login} disabled={loading||pending||!configured}><GoogleMark/>{loading?'Checking sign-in…':pending?'Connecting to Google…':'Continue with Google'}</button>{!loading&&!configured?<div className="auth-setup-note" role="status">Google sign-in is awaiting configuration. Please check back shortly.</div>:null}{error?<p className="error" role="alert">{error}</p>:null}<div className="auth-divider"/><div className="auth-privacy"><Icon name="shield"/><span>We use your Google identity to sign you in.<br/>Your Google password stays with Google.</span></div></div><small className="auth-caption">AI Computing Credit Launchpad</small></section></main>;
}
