"""Operator CLI. Reads chain events and creates private deployment deliveries; never signs transactions."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import time

ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('monitor',ROOT/'frp/server/stake_monitor.py')
monitor=importlib.util.module_from_spec(spec)
spec.loader.exec_module(monitor)
MODEL={'model':'Qwen/Qwen3-8B-AWQ','revision':'93eb32ea9ef98cc266b85563ad0dd29e5f2940b9','precision':'AWQ 4-bit','context':4096}

def main():
    os.umask(0o077)
    parser=argparse.ArgumentParser()
    parser.add_argument('--config',default='.secrets/chain.json')
    parser.add_argument('--server-dir',default='.secrets/frp-server')
    sub=parser.add_subparsers(dest='command',required=True)
    create=sub.add_parser('project');create.add_argument('--owner',required=True);create.add_argument('--name',required=True)
    create.add_argument('--stake-raw',type=int,required=True)
    sub.add_parser('once');sub.add_parser('watch');sub.add_parser('list')
    args=parser.parse_args();config=json.loads(Path(args.config).read_text())
    if type(config.get('deployment_block')) is not int or config['deployment_block']<0:
        raise ValueError('Configure the contract deployment block')
    service=monitor.Monitor(args.server_dir,config)
    if args.command=='project':
        p=service.create_project(args.owner,args.name,MODEL,args.stake_raw)
        print(json.dumps({'project_id':p['id'],'contract_project_key':monitor.project_key(p['id']),
                          'owner':p['owner'],'stake_raw':p['stake_required'],'status':p['status']},indent=2))
    elif args.command=='list':
        for p in service.data()['projects'].values():
            print(json.dumps({k:p[k] for k in ('id','name','owner','status','provider_id','launch_id','verification_deadline') if k in p}))
    elif args.command=='once':
        print('Confirmed stake deliveries:',service.tick())
    else:
        while True:
            print('Confirmed stake deliveries:',service.tick(),flush=True)
            time.sleep(5)

if __name__=='__main__':
    try:main()
    except (ValueError,OSError,KeyError):
        raise SystemExit('Monitor stopped safely; check default-chain config, RPC availability and event/project consistency. No RPC URL or credentials were logged.')
