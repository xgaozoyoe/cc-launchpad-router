import { Auth } from '@auth/core';
import {getToken} from '@auth/core/jwt';
import type {JWT} from '@auth/core/jwt';
import {linkLegacyAccount} from './account-link.ts';
import Google from '@auth/core/providers/google';
import type { AuthConfig } from '@auth/core';

export interface AuthEnvironment {
  CENTRAL_API_ORIGIN?:string;
  CENTRAL_HMAC_KEY?:string;
  GOOGLE_CLIENT_ID?: string;
  GOOGLE_CLIENT_SECRET?: string;
  AUTH_SECRET?: string;
  APP_ORIGIN?: string;
  AUTH_ALLOWED_EMAILS?: string;
  AUTH_ACCESS_MODE?: string;
}
const json=(body:unknown,status=200)=>new Response(JSON.stringify(body),{status,headers:{'Content-Type':'application/json','Cache-Control':'no-store'}});
export function validOrigin(value:string|undefined):string|null {
  try {
    if(!value)return null;
    const url=new URL(value);
    const local=['localhost','127.0.0.1','[::1]'].includes(url.hostname);
    if(url.username||url.password||url.pathname!=='/'||url.search||url.hash||!(url.protocol==='https:'||(url.protocol==='http:'&&local)))return null;
    return url.origin;
  } catch {return null;}
}
export function configured(env:AuthEnvironment):boolean {
  return Boolean(env.GOOGLE_CLIENT_ID?.endsWith('.apps.googleusercontent.com')&&env.GOOGLE_CLIENT_SECRET&&env.AUTH_SECRET&&env.AUTH_SECRET.length>=32&&validOrigin(env.APP_ORIGIN)&&(env.AUTH_ACCESS_MODE==='public'||env.AUTH_ALLOWED_EMAILS?.trim()));
}
export function emailAllowed(email:unknown,verified:unknown,allowed:string|undefined,mode='private'):boolean {
  if(typeof email!=='string'||verified!==true)return false;
  if(mode==='public')return true;
  const list=(allowed??'').split(',').map(v=>v.trim().toLowerCase()).filter(Boolean);
  return list.includes(email.toLowerCase());
}
export function authConfig(env:AuthEnvironment,legacy:JWT|null=null,link=linkLegacyAccount):AuthConfig {
  const origin=validOrigin(env.APP_ORIGIN)!;
  return {
    basePath:'/api/auth',trustHost:true,secret:env.AUTH_SECRET,
    providers:[Google({clientId:env.GOOGLE_CLIENT_ID,clientSecret:env.GOOGLE_CLIENT_SECRET,checks:['pkce','state','nonce'],authorization:{params:{scope:'openid email profile',prompt:'select_account'}}})],
    session:{strategy:'jwt',maxAge:8*60*60},
    pages:{signIn:'/',error:'/'},
    callbacks:{
      signIn({profile}){return emailAllowed(profile?.email,profile?.email_verified,env.AUTH_ALLOWED_EMAILS,env.AUTH_ACCESS_MODE);},
      redirect(){return origin;},
      async jwt({token,account,profile}){
        if(account?.provider==='google'&&profile?.email_verified===true&&typeof account.providerAccountId==='string'){
          const canonical='google:'+account.providerAccountId;
          if(!/^google:[a-zA-Z0-9_.:-]{1,121}$/.test(canonical))throw new Error('Invalid provider identity');
          if(legacy?.sub&&/^[0-9a-f-]{36}$/i.test(legacy.sub)&&typeof legacy.email==='string'&&typeof profile.email==='string'&&legacy.email.toLowerCase()===profile.email.toLowerCase())await link(env,canonical,legacy.sub);
          token.sub=canonical;
        }
        return token;
      },
      session({session,token}){return {...session,user:{...session.user,id:token.sub}};}
    },
    logger:{error(error){console.error('[auth]',error.name);},warn(code){console.warn('[auth]',code);},debug(){}}
  };
}
export async function handleAuth(request:Request,env:AuthEnvironment=process.env):Promise<Response> {
  const path=new URL(request.url).pathname;
  if(path==='/api/auth/status')return json({configured:configured(env)});
  if(!configured(env))return json({error:'Google sign-in is not configured.'},503);
  if(!['GET','POST'].includes(request.method))return json({error:'Method not allowed.'},405);
  const origin=validOrigin(env.APP_ORIGIN)!;
  if(request.method==='POST'&&request.headers.get('origin')!==origin)return json({error:'Invalid request origin.'},403);
  const legacy=await getToken({req:new Request(request.url,{headers:{cookie:request.headers.get('cookie')??''}}),secret:env.AUTH_SECRET!,secureCookie:origin.startsWith('https:')});
  const response=await Auth(request,authConfig(env,legacy));
  response.headers.set('Cache-Control','no-store');
  response.headers.set('X-Content-Type-Options','nosniff');
  return response;
}
