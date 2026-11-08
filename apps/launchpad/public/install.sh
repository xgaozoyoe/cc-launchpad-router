#!/usr/bin/env bash
set -euo pipefail
command -v python3 >/dev/null || { echo 'Python 3 is required. Install it and rerun.' >&2; exit 1; }
# Store the installer in the requested working directory; no root or global changes.
umask 077
mkdir -p .acc-install
curl -fsSL https://riddle.xyz/provider-install.py -o .acc-install/installer.py
exec python3 .acc-install/installer.py "$@"
