#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
if [[ -f .env ]]; then
  read -r saved_gpu saved_port < <(python3 - <<'PYREAD'
from pathlib import Path
values = dict(line.split('=',1) for line in Path('.env').read_text().splitlines() if '=' in line)
print(values.get('GPU_DEVICE','2'), values.get('PORT','8002'))
PYREAD
)
  export GPU_DEVICE="${GPU_DEVICE:-$saved_gpu}" PORT="${PORT:-$saved_port}"
fi
while (($#)); do
  case "$1" in
    --gpu) export GPU_DEVICE="$2"; shift 2 ;;
    --port) export PORT="$2"; shift 2 ;;
    *) echo 'Usage: ./run.sh [--gpu INDEX] [--port PORT]' >&2; exit 1 ;;
  esac
done
[[ ${GPU_DEVICE:-2} =~ ^[0-9]+$ ]] || exit 1
[[ ${PORT:-8002} =~ ^[0-9]+$ ]] && (( ${PORT:-8002} >= 1024 && ${PORT:-8002} <= 65535 )) || exit 1
if [[ ! -f .env ]]; then
  umask 077
  python3 -c 'import secrets; print("INFERENCE_API_KEY="+secrets.token_hex(32))' > .env
fi
# Persist selection so compose status/restart uses the same device and port.
python3 - "${GPU_DEVICE:-2}" "${PORT:-8002}" <<'PYCONFIG'
import sys
from pathlib import Path
p = Path('.env')
values = dict(line.split('=', 1) for line in p.read_text().splitlines() if '=' in line)
values.update(GPU_DEVICE=sys.argv[1], PORT=sys.argv[2])
p.write_text(''.join(k+'='+v+'\n' for k,v in values.items()))
p.chmod(0o600)
PYCONFIG
docker compose up -d
printf 'Vision API starting on localhost:%s. Check: docker compose logs -f inference\n' "${PORT:-8002}"
