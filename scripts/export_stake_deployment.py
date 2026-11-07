#!/usr/bin/env python3
"""Verify Forge's mined deployment and export public chain configuration (no keys)."""
import argparse
import json
import os
from pathlib import Path
import re
import urllib.request

ADDRESS=re.compile(r'^0x[0-9a-fA-F]{40}$')
def address(value):
    if not isinstance(value,str) or not ADDRESS.fullmatch(value):raise ValueError('Invalid contract/role address')
    return value.lower()

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--broadcast',required=True,type=Path)
    p.add_argument('--output',required=True,type=Path)
    p.add_argument('--allow-local',action='store_true')
    args=p.parse_args()
    url=os.environ.get('CHAIN_RPC_URL')
    if not url:raise ValueError('Set CHAIN_RPC_URL')
    def rpc(method,params):
        request=urllib.request.Request(url,data=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode(),headers={'Content-Type':'application/json','User-Agent':'cc-launchpad-deployer/1.0'})
        try:
            with urllib.request.urlopen(request,timeout=30) as response:data=json.load(response)
        except Exception:raise RuntimeError('RPC request failed') from None
        if 'error' in data:raise RuntimeError('RPC rejected '+method)
        return data['result']
    chain=int(rpc('eth_chainId',[]),16)
    if chain!=11155111 and not (args.allow_local and chain==31337):raise ValueError('Expected Sepolia; local Anvil requires --allow-local')
    saved=json.loads(args.broadcast.read_text())
    if saved.get('chain')!=chain:raise ValueError('Broadcast chain mismatch')
    txs=saved['transactions']
    receipts=[]
    for tx in txs:
        receipt=rpc('eth_getTransactionReceipt',[tx['hash']])
        if not receipt or int(receipt['status'],16)!=1:raise ValueError('Deployment transaction not mined successfully')
        block=rpc('eth_getBlockByNumber',[receipt['blockNumber'],False])
        if block['hash'].lower()!=receipt['blockHash'].lower():raise ValueError('Deployment receipt is not canonical')
        receipts.append(receipt)
    if not receipts:raise ValueError('No mined deployment transactions')
    deployed={}
    for tx,receipt in zip(txs,receipts):
        if tx.get('transactionType')=='CREATE' and tx.get('contractName') in ('TestPRL','LaunchStake'):
            name=tx['contractName']
            if name in deployed:raise ValueError('Duplicate contract deployment')
            deployed[name]=address(receipt['contractAddress'])
            if deployed[name]!=address(tx['contractAddress']):raise ValueError('Contract address mismatch')
    if set(deployed)!={'TestPRL','LaunchStake'}:raise ValueError('Expected both TestPRL and LaunchStake')
    token,escrow=deployed['TestPRL'],deployed['LaunchStake']
    for contract in (token,escrow):
        if rpc('eth_getCode',[contract,'latest']) in ('0x','0x0'):raise ValueError('Deployed contract has no code')
    def call(contract,data):return rpc('eth_call',[{'to':contract,'data':data},'latest'])
    if address('0x'+call(escrow,'0x51ed6a30')[-40:])!=token:raise ValueError('Wrong escrow token')
    owner=address('0x'+call(token,'0x8da5cb5b')[-40:])
    admin=address(os.environ.get('STAKE_ADMIN',owner))
    if admin!=owner:raise ValueError('Wrong test token owner')
    verifier=address(os.environ.get('STAKE_VERIFIER',admin))
    settler=address(os.environ.get('STAKE_SETTLER',admin))
    roles=[('0x'+'00'*32,admin),(call(escrow,'0xe7705db6'),verifier),(call(escrow,'0x7445e85c'),settler)]
    for role,who in roles:
        if int(call(escrow,'0x91d14854'+role[2:]+who[2:].rjust(64,'0')),16)!=1:raise ValueError('Missing configured role')
    result={'chain_id':chain,'network':'sepolia' if chain==11155111 else 'anvil','staking_contract':escrow,'token_address':token,'token_kind':'test-only-tPRL','deployment_block':min(int(r['blockNumber'],16) for r in receipts),'batch_size':100,'admin':admin,'verifier':verifier,'settler':settler,'transaction_hashes':[r['transactionHash'] for r in receipts]}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    # Refuse to replace an existing deployment record accidentally.
    with args.output.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
