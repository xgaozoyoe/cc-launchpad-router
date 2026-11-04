import { defineConfig, loadEnv } from 'vite';
import type { Plugin, ViteDevServer, PreviewServer } from 'vite';
import { nodeAuth } from './server/node-adapter';
function localAuth():Plugin {
  const install=(server:ViteDevServer|PreviewServer)=>{
    const env={...process.env,...loadEnv('development',process.cwd(),'')};
    server.middlewares.use('/api/auth',(req,res)=>{req.url='/api/auth'+(req.url??'');void nodeAuth(req,res,env);});
  };
  return {name:'local-google-auth',configureServer:install,configurePreviewServer:install};
}
export default defineConfig({plugins:[localAuth()],build:{outDir:'dist'},server:{port:4173,strictPort:true}});
