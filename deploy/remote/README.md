# User-owned Ubuntu backend deployment

The current remote backend runs natively under the SSH user's account, isolated from existing system services. This avoids requiring Docker socket access or sudo. Rust is built with 1.91.1; MongoDB uses the official Ubuntu 24.04 x86_64 8.0.16 archive, checked against the vendor's SHA-256 file.

Layout under `~/services/cc-launchpad/`:

- `bin/cc-central`: release Rust backend
- `bin/bootstrap-mongo`: localhost-only replica set/application-user initializer
- `private/config.json`: generated database credentials and runtime configuration (0600)
- `private/backend-hmac.key`: server request signing key (0600)
- `private/mongo-key`: MongoDB replication key (0600)
- `private/mongo-data/`: persistent database
- `private/mongo.log`, `private/service.log`: private operational logs
- `deploy/remote/supervise.py`: restart supervisor
- `run.sh`: supervisor launcher guarded by `flock`
- `releases/`: source snapshots

The Node web/session gateway binds to the configured Tailscale IPv4 address on port 9001 and Rust binds to loopback port 9002. MongoDB binds only to `127.0.0.1:27189`, uses authentication and a dedicated read/write application account, and does not reuse the host's existing MongoDB instance. `/healthz` is readable on the tailnet; `/api/v1/*` requires signed server requests.

The supervisor manages MongoDB and Rust, plus Node when private/web.json exists, and restarts the managed stack after a process failure. A user crontab `@reboot` entry invokes `run.sh`; existing cron entries are preserved. The cron daemon must be active. Starting `run.sh` twice is prevented by its private lock. No system services, Docker permissions or firewall rules are changed.

## Operations

```sh
# On the deployment host:
~/services/cc-launchpad/run.sh &
# Health (substitute the host's actual Tailscale address)
curl --fail http://TAILSCALE_IP:9001/healthz
# Logs: inspect locally, do not paste credentials/configuration into public issues
tail -n 40 ~/services/cc-launchpad/private/service.log
```

Before replacing binaries, stop the managed supervisor and wait for its children to exit. Preserve `private/` and back up MongoDB with an authenticated database backup tool before upgrades. Do not delete the database directory or regenerate its existing passwords/replication key.

The bootstrap binary requires `MONGO_PORT`, `MONGO_ROOT_PASSWORD` and `MONGO_APP_PASSWORD` from the private configuration. It initializes only a loopback connection, uses MongoDB's localhost exception to create the first administrator on a fresh authenticated instance, and then creates the `launchpad` application user using authenticated administrator access. Repeating it does not overwrite existing users or passwords.

The deployment now supports drafts, project-bound wallet verification, chain registration and finalized stake-to-Launch-ID issuance. FRP authorization, RPC acceptance and credit sales remain unconnected. The separate Vercel demo and local configuration are not changed by this backend deployment. A Vercel connection still needs a reachable HTTPS origin and its server-only signing key; a Tailscale-only address is reachable only by tailnet members.

Verified on Ubuntu: signed status, rejection of nonce replay, draft persistence, idempotent creation, cross-account 404, repeatable database initialization, and supervisor recovery after terminating the backend process. A host reboot was not performed.

## Enabled UI gateway

The production UI and Auth.js gateway are installed under `web/`. The supervisor also starts this Node process when `private/web.json` exists. It runs with `NODE_ENV=production`, Google authentication and no local test-account bypass. Google credentials are held only in the private configuration.

The existing Caddy upstream on the tailnet's port 9001 now points to the UI gateway. Rust binds privately to `127.0.0.1:9002`. `/healthz` and signed `/api/v1/*` requests pass through to Rust, while `/api/central/*` derives the account from the verified Google session before signing backend requests. The domain root serves React and `/api/auth/*` handles login.

Register `https://riddle.xyz/api/auth/callback/google` in the existing Google OAuth web client's Authorized redirect URIs. Without that entry Google returns `redirect_uri_mismatch`; the website and backend can be healthy while login is blocked. Real Google login and stable account/project persistence have been verified. Existing balances/purchases outside server projects remain simulations.

## Active staking worker

The Rust process now verifies project-bound wallet signatures, registers prepared projects using the dedicated Sepolia operator keystore and indexes finalized stake events into MongoDB. See [the staking integration guide](../../docs/RUST-STAKING.md). New Launch IDs remain `awaiting-rpc`; no credit sales or provider credential bundles are activated yet.

## UI-only updates

The gateway serves React assets from `web/dist`. For a frontend-only release, build and store the artifact inside the project root, copy new assets while retaining previous hashed assets, then atomically replace `index.html` last. Record the source revision in `DEPLOYED_REVISION`. No backend/database restart is needed for static-only changes. This does not update Node authentication code, backend binaries or private environment configuration.

My launches displays account-owned projects and current collateral balances. The test deployment stops at awaiting RPC; no FRPS inference endpoint, buyer sales or progressive collateral release has been activated.
