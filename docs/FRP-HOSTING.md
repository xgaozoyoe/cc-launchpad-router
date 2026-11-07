# Central FRPS and unified launch-ID router

> Separate reference stack, not the active Rust/MongoDB deployment. Read [implementation status](IMPLEMENTATION-STATUS.md). Publishing workflow target: read [Stake-gated project lifecycle](PROJECT-LIFECYCLE.md). Provider ID is now the confirmed stake tx hash; The active Rust worker reserves the Launch ID after finalized stake acceptance; native Rust credential delivery is available via the authenticated [provider bundle download](PROVIDER-BUNDLES.md). Complete RPC acceptance within 24h of the stake block. Manual `frp_admin.py add` / `launch` commands are disabled; they are not production onboarding steps.

FRPC is required for every provider. The platform owns the public API hostname and resolves each **launch ID** to an assigned provider’s FRPC tunnel and exact model profile.

```text
https://api.example.com/launches/launch-001/v1
https://api.example.com/launches/launch-002/v1
```

These are two credit launches on one public hostname, not provider-owned domains. A launch ID identifies a credit issuance; a provider ID identifies a tunnel. Multiple launches can share a provider tunnel for its configured model, subject to capacity controls outside this starter.

## 1. Initialize the central host

Use a public Linux server with Docker, Compose v2, Python 3.11+ and OpenSSL supporting `req -addext`. It does not need a GPU.

- Point `frps.example.com` and `api.example.com` DNS A records at the server. Only add AAAA when IPv6 works. No wildcard provider DNS is needed.
- Allow inbound TCP 7000 for provider tunnels and 80/443 for HTTPS and certificate issuance.
- Do not publicly expose 7500, 8080, 8001 or 9090.

Run in the repository root:

```bash
python3 scripts/frp_admin.py init --server frps.example.com --api-domain api.example.com
docker compose -f frp/server/compose.yaml up -d
```

Use actual hostnames. Initialization starts an empty reference registry; it does not authorize any provider. The production application does not currently populate it.

The stack contains:

| Service | Role |
| --- | --- |
| Caddy | One public HTTPS hostname; forwards launch-scoped API paths to the router |
| Launch router | Resolves launch ID → enabled provider + matching model; proxies via private FRPS HTTP ingress |
| FRPS | Accepts provider FRPC tunnels over verified TLS; routes internal provider Hosts |
| Private registry/plugin | Authorizes tunnel identities and route ownership; lists configured providers and launches |

FRPS uses `providers.internal` solely as a private Host-routing suffix. For stake-backed records, the router uses the registry-assigned hashed alias in `Host: <alias>.providers.internal` to `frps:8080`; the full stake hash is too long for a DNS label. These internal names need no DNS records, are never buyer API endpoints, and cannot be chosen by public callers.

## 2. Deliver the provider bundle

Initialization generates private server configuration in `.secrets/frp-server/`. The separate Python monitor/lifecycle can export `.secrets/frp-server/deliveries/<launch-id>/` after accepted stakes. The active Rust app does not create this directory or expose owner downloads. A reference bundle contains:

- `frpc.toml`: provider identity, credential and tunnel settings.
- `ca.crt`: pinned public FRPS certificate.
- `token`: FRP transport token.

Deliver only that provider’s bundle through a secure channel. Never deliver `server.key`, the registry or another provider’s bundle. The [provider guide](HOSTING.md) explains where to install the bundle and how to run Qwen + FRPC.

The generated tunnel certificate is valid for 365 days; renew and redistribute the public trust certificate before expiry. The starter uses a shared FRP transport token plus an independently checked per-provider credential. A production identity service may replace the shared transport token with per-provider OIDC/mTLS.

## 3. Project registration boundary

Manual `add` and `launch` commands refuse issuance. The separate Python lifecycle can create authenticated registry records after its trusted stake and RPC checks; see [reference monitor details](STAKING-CONTRACT.md). Do not edit JSON to bypass those checks or treat a Rust-issued Launch ID as an already provisioned FRP route.

The reference registry is reread per request. Its configured public URL format is `https://<api-domain>/launches/<launch-id>/v1`; no such inference service is enabled by the current riddle.xyz deployment.

The router accepts only:

```text
GET  /launches/<launch-id>/v1/models
POST /launches/<launch-id>/v1/chat/completions
```

It removes the launch prefix before forwarding to vLLM, forwards SSE as it arrives, and returns `X-Launch-ID` for tracing. Missing/disabled launches or providers and mismatched model profiles are rejected. Provider outages produce an unavailable response; no automatic failover is implemented.

## 4. List and manage entries

On the central host:

```bash
python3 scripts/frp_admin.py list
curl --fail http://127.0.0.1:9090/providers
curl --fail http://127.0.0.1:9090/launches
```

The provider list excludes credentials. Entries are registrations, not proof of live capacity; status is `registered-unverified`. The FRPS dashboard on central-host loopback `127.0.0.1:7500` shows live tunnel/proxy statistics. Its random password is in private `frps.toml`; access it through operator SSH rather than publishing it.

To suspend one launch, set that launch’s `enabled` flag to `false` in `.secrets/frp-server/registry.json` using an operator-only editor. New requests to its URL are rejected without changing other launches. Already-running requests are not interrupted.

To disable a provider, set its `enabled` flag to `false`. The router immediately rejects new requests to all its launches, and the authorization plugin denies new tunnel sessions/routes. Restart FRPS to drop established tunnels if required; that restart briefly disconnects every provider. A future controller should automate audited provider-specific disconnects.

## Authentication and credit accounting

Three layers must remain distinct:

1. FRP transport/provider credentials admit the supplier tunnel and restrict its internal route.
2. Supplier inference credentials authenticate requests to vLLM.
3. Buyer identity and launch-specific credits authorize consumption through the production Launchpad gateway.

The current transport router only forwards a supplier Bearer key for operator smoke testing. It does not implement buyer authentication, a credential vault, credit reservation/burning, metering, PRL staking or failover. Do not give supplier credentials to buyers. Production requires the gateway to check credits for the requested launch ID and inject the supplier key server-side.

The production controller still needs to synchronize the reserved Launch ID, stake-derived Provider ID and credential with this registry, enforce acceptance and activate routing. Rust server projects are not synchronized with it.

## Verification

FRPC/FRPS are pinned to `v0.71.1`. Tests cover credential rejection, provider route ownership, launch-ID/model binding, real local FRP TLS transport, model discovery, API authentication and streamed output through the launch router. The end-to-end test uses a mock model service, not GPU inference. Docker deployment and public ACME certificates are not verified here.

```bash
python3 -m unittest discover -s tests -v
```

To include the real-tunnel test, obtain official matching binaries, verify their checksums and set `FRP_BIN_DIR=/path/to/binaries`. Otherwise that test is skipped.

References: [FRP routing](https://gofrp.org/en/docs/features/http-https/subdomain/), [TLS verification](https://gofrp.org/en/docs/features/common/network/network-tls/), [server plugins](https://gofrp.org/en/docs/features/common/server-plugin/).
