#!/usr/bin/env python3
"""Check the reviewed helper hash against compiled artifacts; --write explicitly repins after review.
Run forge build --root contracts first. Requires the integration venv's eth-utils.
"""
import argparse
import json
import re
from pathlib import Path
from eth_utils import keccak
parser=argparse.ArgumentParser()
parser.add_argument('--write',action='store_true')
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
artifact=json.loads((root/'contracts/out/ModuleDeployer.sol/ModuleDeployer.json').read_text())
expected='0x'+keccak(bytes.fromhex(artifact['deployedBytecode']['object'].removeprefix('0x'))).hex()
source=root/'contracts/src/modular/TestnetProtocolFactory.sol'
text=source.read_text()
pattern=r'(MODULE_DEPLOYER_CODEHASH\s*=\s*)0x[a-fA-F0-9]{64}'
match=re.search(pattern,text)
if not match:raise SystemExit('Missing pinned module deployer hash')
if not args.write and match.group().split('=')[-1].strip()!=expected:raise SystemExit('Module deployer changed: review it, then repin with --write before testing/deploying')
if args.write:source.write_text(re.sub(pattern,lambda m:m.group(1)+expected,text))
for name,words in [('TestnetProtocolFactory',7),('LegacyMigrationGate',4),('ModuleDeployer',0)]:
 data=json.loads(next((root/'contracts/out').glob('*/'+name+'.json')).read_text())
 init=len(data['bytecode']['object'].removeprefix('0x'))//2+32*words
 runtime=len(data['deployedBytecode']['object'].removeprefix('0x'))//2
 if init>49152 or runtime>24576:raise SystemExit(f'{name} exceeds EVM deployment limits')
print('Reviewed helper hash and deployment size limits verified.' if not args.write else 'Helper hash repinned; rebuild and rerun all contract tests.')
