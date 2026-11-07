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

The backend binds to the configured Tailscale IPv4 address on port 9001. MongoDB binds only to `127.0.0.1:27189`, uses authentication and a dedicated read/write application account, and does not reuse the host's existing MongoDB instance. `/healthz` is readable on the tailnet; `/api/v1/*` requires signed server requests.

The supervisor restarts both managed processes after a failure. A user crontab `@reboot` entry invokes `run.sh`; existing cron entries are preserved. The cron daemon must be active. Starting `run.sh` twice is prevented by its private lock. No system services, Docker permissions or firewall rules are changed.

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

This deployment serves project drafts only. Wallet/stake verification, Launch ID issuance, FRP authorization and credit sales remain unconnected. The existing Vercel deployment and local UI configuration are not changed by deploying this backend. A Vercel connection still needs a reachable HTTPS origin and its server-only signing key; a Tailscale-only address is reachable only by tailnet members.

Verified on Ubuntu: signed status, rejection of nonce replay, draft persistence, idempotent creation, cross-account 404, repeatable database initialization, and supervisor recovery after terminating the backend process. A host reboot was not performed.
