# Private provider bundles

The active Rust service can issue a project-specific Linux x86_64 `.tar.gz` bundle after a finalized stake has reserved its Launch ID. Use **My launches → View setup requirements → Download FRPC bundle** while the project is active and within its original stake-based deadline.

The authenticated Node gateway derives the owner from the Google session. Bundle creation requires a same-origin POST. Rust checks owner/legacy aliases, finalized project evidence, non-halted chain checkpoint, current contract owner/amount/state and the latest chain timestamp before and after building. Unknown/other-owner projects return 404; expired/refunded/ineligible projects cannot download. Repeated downloads keep the same project credential; they do not extend the deadline.

## Archive contents and use

```text
provider-<launch-id>/
  bin/frpc
  config/frpc.toml
  certs/ca.crt
  private/token
  private/transport-token
  start.sh
  stop.sh
  manage.py
  README.md
  LICENSE.frp
  manifest.json
```

FRPC 0.71.1 is bundled from its official Linux amd64 release. The operator pins the upstream archive digest and extracted binary checksum. The manifest records file checksums. Archive directories are mode 0700; private/config files are 0600. Keep the entire archive and extracted directory private. Credentials are not stored in public assets or returned by ordinary project APIs. Only their digest is stored in MongoDB `provider_deployments`; the reusable secret is kept in the operator's private credential directory.

On the provider machine, extract with `umask 077`, enter the extracted directory and run `./start.sh`. Python 3 and Linux `/proc` are required. The script verifies configuration, serializes start/stop operations, avoids duplicate starts and records process start time to avoid signalling a reused PID. `./stop.sh` targets only this bundle. Inspect `logs/frpc.log`; process startup alone does not establish authenticated login or acceptance. Automatic reboot startup is not installed.

This profile forwards to a **native model API at 127.0.0.1:8000**. It is not the Docker `inference:8000` profile. The inference Bearer key is separate and is not in the bundle. The public endpoint is `riddle.xyz:7000`. TCP forwarding was verified from the operator and GPU-provider networks after the operator configured port forwarding. TLS and project authorization are additional checks; TCP reachability alone does not authenticate a provider.

## Existing FRPS authorization

The deployment reuses the existing dedicated FRPS endpoint, not a second port. Its project-scoped configuration adds a pinned certificate and the Rust `/internal/frp/handler` hook. The hook is not exposed through the browser proxy. Both Login and NewProxy require the matching project credential, Provider ID and active chain state/deadline. NewProxy additionally restricts the type to HTTP, the proxy name to `<provider-id>.inference` and its custom domain to the assigned deterministic internal alias. Other domains, groups, path routes and identity substitutions are rejected.

The shared FRP transport token and independent project credential are different checks. A valid transport token alone cannot register a project tunnel. HTTP ingress binds to loopback for operator routing; no public buyer route or sales are activated. Login and proxy-registration timestamps are historical observations, not heartbeat monitoring. Existing sessions are not forcibly disconnected at expiry; platform acceptance and new registrations remain gated. Credential rotation/revocation UI is not implemented.

## Backend configuration and API

`PROVIDER_BUNDLE_CONFIG` is a server-only JSON path. It includes the helper/Python paths, pinned FRPC binary/checksum/license, CA path, transport token path, private credential directory, FRPS address/port and TLS server name. `deploy/remote/setup-provider-bundles.py` prepares it using the existing dedicated FRPS token; the original FRPS config is preserved. It does not start or restart services. The existing system frps.service continues to supervise FRPS. Activating its prepared TLS/plugin configuration requires replacing its existing config and restarting that service, separately from the Rust/Node deployment.

- `GET /api/v1/projects/<id>/deployment`: owner-only eligibility, endpoint and last authenticated observations; no secrets.
- `POST /api/v1/projects/<id>/deployment/bundle`: owner-checked private archive payload for the session gateway.
- Browser equivalents use `/api/central/…`. The gateway emits `application/gzip`, attachment disposition and `Cache-Control: no-store, private`.

Functional [Test RPC checks](RPC-CHECKS.md) can now run and save owner-visible results. Formal model/capacity acceptance, persistent inference credential custody, buyer credit accounting and sales remain separate unfinished integrations. Downloading a bundle or establishing a tunnel does not verify capacity or mark a launch live.
