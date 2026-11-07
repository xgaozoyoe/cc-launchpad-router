#!/usr/bin/env python3
"""User-owned backend supervisor; run under flock to prevent duplicate instances."""
import json
import os
from pathlib import Path
import signal
import subprocess
import time

base=Path(__file__).resolve().parents[2]
config=json.loads((base/'private/config.json').read_text())
env={**os.environ,**config}
children=[]
def stop(_signal,_frame):
    raise KeyboardInterrupt
signal.signal(signal.SIGTERM,stop)
signal.signal(signal.SIGINT,stop)
try:
    while True:
        try:
            mongo=subprocess.Popen([str(base/'tools/mongodb-linux-x86_64-ubuntu2404-8.0.16/bin/mongod'),'--dbpath',str(base/'private/mongo-data'),'--replSet','rs0','--port',env['MONGO_PORT'],'--bind_ip','127.0.0.1','--auth','--keyFile',str(base/'private/mongo-key'),'--logpath',str(base/'private/mongo.log'),'--logappend'],env=env)
            children=[mongo]
            subprocess.run([str(base/'bin/bootstrap-mongo')],env=env,check=True)
            backend=subprocess.Popen([str(base/'bin/cc-central')],env=env)
            children.append(backend)
            while all(child.poll() is None for child in children):
                time.sleep(2)
        except Exception as exc:
            print('Backend startup failed:',type(exc).__name__,flush=True)
        finally:
            for child in reversed(children):
                if child.poll() is None:child.terminate()
            for child in children:
                try:child.wait(timeout=30)
                except subprocess.TimeoutExpired:child.kill()
            children=[]
        time.sleep(5)
except KeyboardInterrupt:
    pass
