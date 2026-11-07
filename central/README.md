# Rust central service

The central backend now starts with **Rust, Axum, Tokio and MongoDB**. Use `python3 scripts/stack.py local` from the repository root to run it with the React UI and a local MongoDB replica set. This is a draft-project API foundation, not a deployed replacement for the existing Python stake monitor/FRP services. It does not issue Launch IDs, accept stakes, authorize FRPC, sell credits, or route paid inference yet. Those endpoints are intentionally absent. `status` reports these integration gaps explicitly.

## Run

Use Rust stable (tested with 1.91.1), and an authenticated MongoDB replica set or Atlas. Standalone MongoDB is rejected so later stake/event transactions cannot accidentally run on an unsupported topology. MongoDB is external to this compose file; do not expose its database port publicly.

From the repository root:

```sh
mkdir -p .secrets
python3 -c 'import secrets; from pathlib import Path; p=Path(".secrets/backend-hmac.key"); p.touch(exist_ok=False); p.write_text(secrets.token_hex(32)+"\n"); p.chmod(0o600)'
cp central/.env.example .secrets/central.env
# Edit central.env with your MongoDB URI; URL-encode password characters.
# Container user 10001 needs read access to the mounted key: use a host ACL
# or a managed container secret. Never make the key world-readable.
docker compose -f central/compose.yaml up --build -d
```

For native development, export `MONGODB_URI`, optionally `MONGODB_DATABASE`, and `CENTRAL_HMAC_KEY_FILE` (absolute key path), then run:

```sh
cargo +stable run --manifest-path central/Cargo.toml
```

The default listener is `127.0.0.1:9001`. Put HTTPS and request rate limits in front of it before remote access. The key is server-only: the browser must never receive it. Vercel must validate the Google session and sign requests using the verified session's user ID. The integrated single-host session gateway is now available in `apps/launchpad/server/central.ts`; see `deploy/stack/README.md`. The Vercel deployment continues to use its existing demo flow.

## API and signing

Public `GET /healthz` checks MongoDB availability. All `/api/v1` requests require these headers:

- `X-CC-User`: verified Google subject ID
- `X-CC-Timestamp`: current Unix seconds, accepted within 60 seconds
- `X-CC-Nonce`: fresh 16 bytes encoded as 32 hex characters
- `X-CC-Signature`: hex HMAC-SHA256 using the decoded 32-byte key

Sign the following exact string, with newline separators and no trailing newline:

```text
METHOD
/api/v1/path
USER
TIMESTAMP
NONCE
SHA256_BODY_HEX
```

Queries are not part of this API. Use the exact path without query parameters. Each valid envelope can be used once; a MongoDB unique nonce insert rejects replay across service instances. The TTL index cleans old records, but is not the authorization check. Failed database access fails closed. Responses use `Cache-Control: no-store`.

Endpoints:

| Method | Path | Result |
| --- | --- | --- |
| GET | `/api/v1/status` | Integration status; sales disabled |
| GET | `/api/v1/models` | Supported Qwen model profile |
| POST | `/api/v1/projects` | Create draft with `name`, `model_id`, UUID `request_id` |
| GET | `/api/v1/projects` | Current account's latest 100 projects |
| GET | `/api/v1/projects/<id>` | Owned project; otherwise 404 |

A repeated `request_id` returns the original draft; changing its parameters returns 409. The request cannot supply status, owner, wallet verification, Launch ID, or provider credentials. The draft's internal UUID is not a Launch ID.

Collections: `projects` has a unique `(account_id, request_id)` index; `request_nonces` has a unique `_id` and expiry TTL index. PRL balances/events/credits are not stored by this foundation yet. Backup MongoDB and protect the HMAC key separately.

## Verification and next integration

```sh
cargo +stable test --manifest-path central/Cargo.toml
```

Before replacing `frp/server/compose.yaml`, port wallet ownership validation, finalized Sepolia event indexing, atomic stake-to-Launch-ID issuance and credential delivery, FRP authorization, and inference routing to MongoDB. Do not run a SQLite writer and a MongoDB writer against the same project lifecycle. The existing Python services remain unchanged as references until that migration is verified.

The integration test additionally uses a disposable local MongoDB replica set on port 27187:

```sh
# Start mongod with a fresh temporary dbpath, --replSet rs0 and --bind_ip 127.0.0.1.
# This test initializes that test-only replica set, creates a random database,
# and drops the test database after checking persistence, ownership and replay.
cargo +stable test --manifest-path central/Cargo.toml -- --ignored
```

Verified locally: four signing unit tests and the real MongoDB integration test pass. A container image build and public-host deployment have not been verified.
