import type { IncomingMessage, ServerResponse } from 'node:http';
import { handleAuth, validOrigin } from './auth.js';
import type { AuthEnvironment } from './auth.ts';
type NodeRequest=IncomingMessage&{body?:unknown};

export async function nodeAuth(req:NodeRequest,res:ServerResponse,env:AuthEnvironment=process.env):Promise<void> {
  try {
    const origin=validOrigin(env.APP_ORIGIN)??'http://localhost:4173';
    const url=new URL(req.url??'/api/auth/status',origin);
    // The deployment rewrite carries the original action in a query parameter.
    if(url.pathname==='/api/auth'&&url.searchParams.has('authPath')){
      url.pathname='/api/auth/'+url.searchParams.get('authPath');url.searchParams.delete('authPath');
    }
    const headers=new Headers();
    for(const [key,value] of Object.entries(req.headers)){
      if(value!==undefined)headers.set(key,Array.isArray(value)?value.join(', '):value);
    }
    let body:string|undefined;
    if(req.method==='POST'){
      if(typeof req.body==='string')body=req.body;
      else if(req.body&&typeof req.body==='object'){
        body=headers.get('content-type')?.includes('application/x-www-form-urlencoded')?new URLSearchParams(req.body as Record<string,string>).toString():JSON.stringify(req.body);
      }else{
        const chunks:Buffer[]=[];let length=0;
        for await(const chunk of req){const buffer=Buffer.isBuffer(chunk)?chunk:Buffer.from(chunk);length+=buffer.length;if(length>16384){res.writeHead(413);res.end();return;}chunks.push(buffer);}
        body=Buffer.concat(chunks).toString('utf8');
      }
      if(Buffer.byteLength(body??'')>16384){res.writeHead(413);res.end();return;}
    }
    const response=await handleAuth(new Request(url,{method:req.method??'GET',headers,body}),env);
    res.statusCode=response.status;
    response.headers.forEach((value,key)=>{if(key!=='set-cookie')res.setHeader(key,value);});
    const cookies=response.headers.getSetCookie();if(cookies.length)res.setHeader('Set-Cookie',cookies);
    res.end(Buffer.from(await response.arrayBuffer()));
  }catch{
    res.statusCode=500;res.setHeader('Content-Type','application/json');res.setHeader('Cache-Control','no-store');res.end(JSON.stringify({error:'Sign-in is temporarily unavailable.'}));
  }
}
