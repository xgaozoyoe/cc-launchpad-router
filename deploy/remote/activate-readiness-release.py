#!/usr/bin/env python3
"""Activate only the reviewed cutover-readiness application release, never chain configuration.
Run under /home/xgao/services/cc-launchpad after staging/build/test.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import time
import urllib.request

BASE = Path('/home/xgao/services/cc-launchpad')
os.umask(0o077)
parser = argparse.ArgumentParser()
parser.add_argument('--stage', required=True)
parser.add_argument('--revision', required=True)
args = parser.parse_args()
if not re.fullmatch(r'[0-9a-f]{40}', args.revision):
    raise SystemExit('Expected full reviewed revision')
stage = (BASE / args.stage).resolve()
if stage.parent != BASE / 'releases' or not stage.is_dir():
    raise SystemExit('Expected a direct project release directory')
for file in [stage / 'web/dist/index.html', stage / 'web/.server-check/server/standalone.js', BASE / 'build/stake/release/cc-central']:
    if not file.is_file():
        raise SystemExit('Missing staged artifact')
# Configurations must remain byte-for-byte unchanged. No secret contents are printed.
configs = [BASE / 'private/config.json', BASE / 'private/web.json', BASE / 'deploy/sepolia.json', BASE / 'deploy/credits-sepolia.json']
def fingerprints():
    return {str(p.relative_to(BASE)): hashlib.sha256(p.read_bytes()).hexdigest() for p in configs}
before = fingerprints()
backup = BASE / 'private' / ('readiness-backup-' + time.strftime('%Y%m%dT%H%M%SZ', time.gmtime()))
backup.mkdir(exist_ok=False)
for name in ['bin/cc-central', 'web/.server-check', 'web/dist/index.html', 'DEPLOYED_REVISION']:
    src = BASE / name
    if not src.exists():
        continue
    dst = backup / name
    dst.parent.mkdir(parents=True, exist_ok=True)
    if src.is_dir():
        shutil.copytree(src, dst)
    else:
        shutil.copy2(src, dst)
(backup / 'configuration-fingerprints.json').write_text(json.dumps(before, indent=2))

def stop():
    lines = subprocess.check_output(['ps', '-u', 'xgao', '-o', 'pid=,args='], text=True).splitlines()
    pids = []
    for line in lines:
        pid, _, command = line.strip().partition(' ')
        if command.strip() == '/usr/bin/python3 deploy/remote/supervise.py':
            pids.append(int(pid))
            os.kill(int(pid), signal.SIGTERM)
    for _ in range(45):
        live = []
        for pid in pids:
            try:
                os.kill(pid, 0)
                live.append(pid)
            except ProcessLookupError:
                pass
        if not live:
            return
        time.sleep(1)
    raise RuntimeError('Project supervisor did not stop gracefully')

def start():
    with open(BASE / 'private/service.log', 'ab') as log:
        subprocess.Popen([str(BASE / 'run.sh')], cwd=BASE, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)

def healthy():
    for _ in range(45):
        try:
            with urllib.request.urlopen('http://127.0.0.1:9002/healthz', timeout=2) as response:
                if json.load(response).get('status') == 'ok':
                    return
        except (OSError, ValueError):
            pass
        time.sleep(1)
    raise RuntimeError('Project health check failed')

stop()
try:
    shutil.copy2(BASE / 'build/stake/release/cc-central', BASE / 'bin/cc-central.next')
    os.replace(BASE / 'bin/cc-central.next', BASE / 'bin/cc-central')
    shutil.copytree(stage / 'web/.server-check', BASE / 'web/.server-check', dirs_exist_ok=True)
    for item in (stage / 'web/dist').iterdir():
        if item.name == 'index.html':
            continue
        if item.is_dir():
            shutil.copytree(item, BASE / 'web/dist' / item.name, dirs_exist_ok=True)
        else:
            shutil.copy2(item, BASE / 'web/dist' / item.name)
    shutil.copy2(stage / 'web/dist/index.html', BASE / 'web/dist/index.next.html')
    os.replace(BASE / 'web/dist/index.next.html', BASE / 'web/dist/index.html')
    if fingerprints() != before:
        raise RuntimeError('Unexpected chain or service configuration change')
    start()
    healthy()
    (BASE / 'DEPLOYED_REVISION').write_text(args.revision + '\n')
except Exception:
    stop()
    shutil.copy2(backup / 'bin/cc-central', BASE / 'bin/cc-central')
    shutil.copytree(backup / 'web/.server-check', BASE / 'web/.server-check', dirs_exist_ok=True)
    shutil.copy2(backup / 'web/dist/index.html', BASE / 'web/dist/index.html')
    if (backup / 'DEPLOYED_REVISION').exists():
        shutil.copy2(backup / 'DEPLOYED_REVISION', BASE / 'DEPLOYED_REVISION')
    start()
    raise
print('Readiness release healthy; chain/service configuration unchanged; MongoDB preserved.')
print('Recovery artifacts: ' + str(backup))
