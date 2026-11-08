#!/usr/bin/env python3
"""Atomically replace static UI assets only. Backend, auth and chain configuration stay untouched."""
import argparse,hashlib,json,os,re,shutil,time,urllib.request
from pathlib import Path
BASE=Path('/home/xgao/services/cc-launchpad')
p=argparse.ArgumentParser();p.add_argument('--stage',required=True);p.add_argument('--revision',required=True);args=p.parse_args()
stage=(BASE/args.stage).resolve()
if stage.parent!=BASE/'releases' or not re.fullmatch('[0-9a-f]{40}',args.revision):raise SystemExit('Invalid release identity')
source=stage/'web/dist';target=BASE/'web/dist'
if not (source/'index.html').is_file():raise SystemExit('Missing built UI')
protected=[BASE/'private/config.json',BASE/'private/web.json',BASE/'deploy/sepolia.json',BASE/'deploy/credits-sepolia.json',BASE/'bin/cc-central']
def fingerprints():return {str(f):hashlib.sha256(f.read_bytes()).hexdigest()for f in protected}
before=fingerprints();backup=BASE/'private'/('ui-backup-'+time.strftime('%Y%m%dT%H%M%SZ',time.gmtime()));backup.mkdir(mode=0o700)
shutil.copy2(target/'index.html',backup/'index.html');shutil.copy2(BASE/'DEPLOYED_REVISION',backup/'DEPLOYED_REVISION')
try:
 for item in source.iterdir():
  if item.name=='index.html':continue
  if item.is_dir():shutil.copytree(item,target/item.name,dirs_exist_ok=True)
  else:shutil.copy2(item,target/item.name)
 shutil.copy2(source/'index.html',target/'index.next.html');os.replace(target/'index.next.html',target/'index.html')
 if fingerprints()!=before:raise RuntimeError('Unexpected backend or configuration change')
 with urllib.request.urlopen('http://127.0.0.1:9002/healthz',timeout=10)as response:
  if json.load(response).get('status')!='ok':raise RuntimeError('Backend unhealthy')
 (BASE/'DEPLOYED_REVISION').write_text(args.revision+'\n')
except Exception:
 shutil.copy2(backup/'index.html',target/'index.next.html');os.replace(target/'index.next.html',target/'index.html');shutil.copy2(backup/'DEPLOYED_REVISION',BASE/'DEPLOYED_REVISION');raise
print('UI updated atomically; backend/config fingerprints unchanged; no service restart.')
