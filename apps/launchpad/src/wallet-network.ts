export type NetworkWallet={request:(args:{method:string;params?:unknown[]})=>Promise<unknown>};
export async function ensureWalletChain(wallet:NetworkWallet,chainId:number):Promise<void>{
 if(!Number.isSafeInteger(chainId)||chainId<=0)throw new Error('The backend returned an invalid target chain.');
 const target='0x'+chainId.toString(16);
 const matches=(value:unknown)=>typeof value==='string'&&/^0x[0-9a-f]+$/i.test(value)&&BigInt(value)===BigInt(chainId);
 if(matches(await wallet.request({method:'eth_chainId'})))return;
 try{
  try{await wallet.request({method:'wallet_switchEthereumChain',params:[{chainId:target}]});}
  catch(error){
   if((error as {code?:number}).code!==4902)throw error;
   if(chainId!==11155111)throw new Error('Add the configured network (chain '+chainId+') to your wallet, then connect again.');
   await wallet.request({method:'wallet_addEthereumChain',params:[{chainId:target,chainName:'Ethereum Sepolia',nativeCurrency:{name:'Sepolia Ether',symbol:'ETH',decimals:18},rpcUrls:['https://ethereum-sepolia-rpc.publicnode.com'],blockExplorerUrls:['https://sepolia.etherscan.io']}]});
   if(!matches(await wallet.request({method:'eth_chainId'})))await wallet.request({method:'wallet_switchEthereumChain',params:[{chainId:target}]});
  }
 }catch(error){if((error as {code?:number}).code===4001)throw new Error('Network switch cancelled. Connect again and approve the requested network to continue.');throw error;}
 if(!matches(await wallet.request({method:'eth_chainId'})))throw new Error('Wallet did not switch to the configured network. Connect again.');
}
