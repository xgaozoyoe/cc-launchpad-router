export interface Account {id:string;name:string;email:string;image?:string}
export async function readSession():Promise<Account|null> {
  const response=await fetch('/api/auth/session',{credentials:'same-origin',cache:'no-store'});
  if(!response.ok)throw new Error('We could not check your session. Please try again.');
  const session:unknown=await response.json();
  if(!session||typeof session!=='object'||!('user' in session))return null;
  const user=session.user;
  if(!user||typeof user!=='object'||!('id' in user)||typeof user.id!=='string'||!('email' in user)||typeof user.email!=='string')return null;
  return {id:user.id,email:user.email,name:'name' in user&&typeof user.name==='string'?user.name:user.email,image:'image' in user&&typeof user.image==='string'?user.image:undefined};
}
async function authAction(path:'signin/google'|'signout'):Promise<string> {
  const csrfResponse=await fetch('/api/auth/csrf',{credentials:'same-origin',cache:'no-store'});
  if(!csrfResponse.ok)throw new Error('Sign-in is temporarily unavailable. Please try again.');
  const data:unknown=await csrfResponse.json();
  if(!data||typeof data!=='object'||!('csrfToken' in data)||typeof data.csrfToken!=='string')throw new Error('Please refresh this page and try again.');
  const response=await fetch('/api/auth/'+path,{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/x-www-form-urlencoded','X-Auth-Return-Redirect':'1'},body:new URLSearchParams({csrfToken:data.csrfToken,callbackUrl:location.origin})});
  const result:unknown=await response.json();
  if(!response.ok||!result||typeof result!=='object'||!('url' in result)||typeof result.url!=='string')throw new Error('Sign-in could not start. Please try again.');
  const url=new URL(result.url,location.origin);
  if(url.origin!==location.origin&&url.origin!=='https://accounts.google.com')throw new Error('The sign-in destination was not recognized.');
  return url.href;
}
export async function signInGoogle(){location.assign(await authAction('signin/google'));}
export async function signOut(){await authAction('signout');}
