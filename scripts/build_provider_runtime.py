"""Rebuild the small public runtime archive from reviewed model source files."""
import io,tarfile
from pathlib import Path
root=Path(__file__).resolve().parents[1]
with tarfile.open(root/'apps/launchpad/public/provider-runtime.tar.gz','w:gz') as archive:
    for name in ['Dockerfile','scripts/serve.py','scripts/healthcheck.py','compose.yaml']:
        data=(root/name).read_bytes()
        if name=='compose.yaml':data=data.replace(b'model-cache:/root/.cache/huggingface',b'./model-cache:/root/.cache/huggingface').replace(b'volumes:\n  model-cache:\n',b'').replace(b'count: 1', b'device_ids: ["${GPU_DEVICE:-0}"]')
        entry=tarfile.TarInfo(name);entry.size=len(data);entry.mode=0o600
        archive.addfile(entry,io.BytesIO(data))
