# Integrated single-host launchpad

One origin serves React, Google login and `/api/central/*`. The web/session process signs the authenticated account ID and forwards requests to Rust on a private address; Rust persists drafts in MongoDB. The browser never needs the central server IP or signing key.

## Local: one command

Prerequisites: Node.js 22+ (tested with 23.1), npm, Rust stable (tested with 1.91.1), Python 3 and MongoDB `mongod` in PATH (Homebrew MongoDB is also detected).

From the repository root:

```sh
python3 scripts/stack.py local
```

Open **http://localhost:4174**, choose **Launch credits**, enter a project name and save. Refresh, reopen the dialog and see the same project from MongoDB. The script installs missing frontend dependencies, builds both services, initializes a local replica set and starts all three processes. Ctrl+C stops them; project data persists under `.secrets/stack/mongo-data`. Subsequent runs use the same configuration and data.

This explicit local test mode signs in a fixed test account. Both web and MongoDB bind to loopback. The web server rejects non-loopback requests in this mode and refuses to enable the test account under `NODE_ENV=production` or a non-local APP_ORIGIN. Local database authentication is disabled for this development-only data. Do not use production data or expose these ports via tunnels/proxies.

Logs and credentials are under `.secrets/stack/`, ignored by Git. Initial native compilation can take several minutes. Stop other processes on ports 4174, 9001 or 27188 before starting. Existing launchpad demo on 4173 can remain running.

## Single public server: Docker Compose

Use a separate clean checkout for production configuration. Prerequisites: Docker Engine with the Compose v2 plugin, Python 3, a domain pointing to the host, open inbound 80/443, and your Google OAuth web client.

```sh
python3 scripts/stack.py setup --origin https://launchpad.example.com
# Edit .secrets/stack/stack.env locally:
# GOOGLE_CLIENT_ID=...
# GOOGLE_CLIENT_SECRET=...
python3 scripts/stack.py up
```

The setup helper generates the database passwords, MongoDB replication key, Auth.js secret and server signing key. It never overwrites an existing configuration. Store the Google secret directly in the private file; do not commit it. In Google Auth Platform add the exact callback:

```text
https://launchpad.example.com/api/auth/callback/google
```

Caddy terminates HTTPS. Only 80/443 are published; web, Rust and MongoDB are private Compose services. MongoDB uses a persistent authenticated single-node replica set with a dedicated `readWrite` application user. Single-node replication enables transactions but provides no redundancy. The initialization job runs after the final authenticated MongoDB process is ready. Application containers run as UID 10001; the signing key is copied to a private volume with matching ownership.

`AUTH_ACCESS_MODE=public` accepts verified Google accounts; Google OAuth's publishing/test-user configuration still applies. To restrict access, use `AUTH_ACCESS_MODE=private` and `AUTH_ALLOWED_EMAILS=comma,separated,emails`. No test-account bypass is enabled in Compose.

```sh
# Status and private logs (may contain infrastructure diagnostics)
docker compose --env-file .secrets/stack/stack.env -f deploy/stack/compose.yaml ps
# Stop services, retaining database and signing-key volumes
python3 scripts/stack.py down
```

Keep the private configuration and volume backups. Changing passwords in the env file does not rotate users already stored in MongoDB; use an explicit database credential rotation. Do not run `down -v` against data you need.

## What is connected

- React → same-origin session gateway → signed Rust API → MongoDB.
- Draft creation, supported Qwen profile, account-isolated listing/details and idempotent retries.
- Existing credit balances, purchases and resources outside this dialog remain clearly labeled concept simulations.

Stake/wallet verification, finalized event indexing, Launch ID credentials, FRPS routing and credit settlement are **not connected to the new Rust API yet**. No stake or sales actions are enabled by this stack. The previous Python FRP kit remains separate; do not expose it as if this draft API had authorized suppliers. This stack establishes the UI/backend/database path first.

## Verification

```sh
npm --prefix apps/launchpad test
npm --prefix apps/launchpad run build
cargo +stable test --manifest-path central/Cargo.toml --locked
```

The local native stack was tested in a browser: create a project, reload and reopen the identical project from MongoDB. Proxy tests check session-derived identity, spoofed headers, cross-origin POST rejection, endpoint allowlisting and unavailable-backend errors. Docker Compose execution and real Google OAuth on a new domain require a running Docker engine and domain/Google configuration; they were not exercised by the native local test.
