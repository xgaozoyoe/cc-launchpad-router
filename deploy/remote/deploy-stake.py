#!/usr/bin/env python3
"""Dedicated Sepolia deployment. Defaults to simulation; --broadcast signs testnet transactions."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import urllib.request

base=Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser()
parser.add_argument('--broadcast',action='store_true')
args=parser.parse_args()
wallet=base/'private/sepolia-wallet'
address=(wallet/'address.txt').read_text().strip()
rpc_url=os.environ.get('CHAIN_RPC_URL','https://ethereum-sepolia-rpc.publicnode.com')
def rpc(method,params):
    req=urllib.request.Request(rpc_url,data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),headers={'Content-Type':'application/json','User-Agent':'cc-launchpad-deployer/1.0'})
    with urllib.request.urlopen(req,timeout=30) as response:result=json.load(response)
    if 'error' in result:raise RuntimeError('Sepolia RPC rejected request')
    return result['result']
if int(rpc('eth_chainId',[]),16)!=11155111:raise SystemExit('Wrong chain: expected Ethereum Sepolia')
release=base/'releases/stake-deploy'
manifest=base/'deploy/sepolia.json'
broadcast=release/'contracts/broadcast/DeploySepolia.s.sol/11155111/run-latest.json'
if manifest.exists():raise SystemExit('A deployment manifest exists; refusing duplicate deployment')
if args.broadcast and broadcast.exists():raise SystemExit('Prior broadcast exists; inspect receipts and resume explicitly before deploying again')
balance=int(rpc('eth_getBalance',[address,'latest']),16)
print('Sepolia deployer:',address,'balance:',format(balance/10**18,'.8f'),'test ETH',flush=True)
if args.broadcast and balance==0:raise SystemExit('Fund this wallet with Sepolia ETH first')
env={**os.environ,'CHAIN_RPC_URL':rpc_url,'DEPLOYER_ADDRESS':address,'ALLOW_LOCAL_DEPLOY':'false'}
for key in ('STAKE_ADMIN','STAKE_VERIFIER','STAKE_SETTLER','TEST_PRL_INITIAL_SUPPLY'):
    env.pop(key,None)
command=[str(base/'tools/foundry/forge'),'script','--root','contracts','contracts/script/DeploySepolia.s.sol:DeploySepolia','--rpc-url',rpc_url,'--sender',address,'--use',str(base/'tools/solc/solc-0.8.30'),'--no-storage-caching','--gas-estimate-multiplier','1000']
if args.broadcast:command+=['--keystore',str(wallet/'keystores/sepolia-deployer'),'--password-file',str(wallet/'password'),'--broadcast','--slow']
subprocess.run(command,cwd=release,env=env,check=True)
if args.broadcast:
    subprocess.run(['python3','scripts/export_stake_deployment.py','--broadcast',str(broadcast),'--output',str(manifest)],cwd=release,env=env,check=True)
    print('Verified public deployment manifest:',manifest)
else:
    print('Simulation only. No Sepolia transactions sent.')
