"""Create a local secret without printing it or replacing existing settings."""
import os
from pathlib import Path
import secrets

root = Path(__file__).resolve().parents[1]
content = (root / '.env.example').read_text().replace(
    'INFERENCE_API_KEY=\n', 'INFERENCE_API_KEY=' + secrets.token_urlsafe(32) + '\n')
try:
    fd = os.open(root / '.env', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
except FileExistsError:
    raise SystemExit('.env already exists; settings were preserved.')
with os.fdopen(fd, 'w') as output:
    output.write(content)
print('Created .env (owner-only permissions). Start with docker compose up --build -d.')
