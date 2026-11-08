# Real model prompt trials

Signed-in users open **Try a model** or **Try a prompt** on a real Launchpad card. `/#try/<project-uuid>` is a direct link. The page sends a short prompt to that project's actual inference API through its assigned FRPC tunnel. It is a free preview: no payment, credit balance, burn or settlement is performed.

## Availability and provider consent

Only a project with finalized `accepted` status, an active matching stake, finalized on-chain Verified state, the supported Qwen3 8B AWQ profile, and explicit provider trial consent can be used. Administrative recheck authorization alone does not enable user trials. The owner first saves a platform test credential, then enables **limited user prompt trials** in provider setup. The existing hosted test provider may be enabled by its operator. The consent flag defaults to false for old credential files.

Owner-only `POST /api/v1/projects/<id>/deployment/rpc-credential` accepts `{trial_enabled:true}` or `{trial_enabled:false}`. No API key is returned. Turning trials off prevents new trial requests; a request already generating may finish. Revoking the stored credential disables both trials and future admin rechecks. The credential remains in the existing private 0600 project-bound file under a 0700 directory. File custody is not application-level encryption; see [credential security](ADMIN-REVIEW.md#admin-rpc-rechecks).

## Limits enforced by Rust and MongoDB

| Limit | Policy |
|---|---|
| Account frequency | 2 attempts per UTC minute window, across all projects |
| Account daily quota | 10 attempts per UTC day, across all projects |
| Shared project frequency | 30 attempts per UTC hour |
| Shared project daily quota | 200 attempts per UTC day |
| Concurrency | 1 trial per account and 1 per project |
| Prompt | Non-empty, at most 500 Unicode characters and 2,000 UTF-8 bytes |
| Generation | Fixed 128 output-token cap, fixed model/system settings, no streaming |
| Timeout/body bound | 45-second generation timeout; provider response at most 64 KiB |

Unique MongoDB `_id` updates atomically enforce each quota ceiling across processes. Window keys and counters persist across refreshes, new browsers and backend restarts. Google/legacy identities resolve to the canonical account before quota enforcement. Quotas are account based; shared project limits also cap aggregate demand from multiple accounts.

Quota reservations are conservative and not refunded. Provider failures/timeouts count. If a later shared quota is exhausted, earlier account reservations may also count. Invalid prompt/body requests and requests rejected by an existing lease do not reserve quotas. TTL indexes clean expired counters and request metadata; eligibility and expiry checks do not depend on TTL cleanup running promptly.

Distributed leases expire after 90 seconds if a process dies, and normal requests release only leases bearing their own request ID. A project cannot be selected by arbitrary URL. A UUID `request_id` is bound to the canonical user and prevents a duplicate from generating again; previous answer content is not stored or replayed. The browser never automatically retries generation. Rate errors return HTTP 429 with `retry_after_seconds` in JSON; the page exposes the user's own remaining quota and local display of reset times.

## API

- `GET /api/v1/launchpad/<project-uuid>/trial`: current project/model metadata, server limits and the signed-in user's remaining allowance.
- `POST /api/v1/launchpad/<project-uuid>/trial`: `{prompt,request_id}` only. Returns sanitized text, model/project IDs, reported token usage, finish reason, elapsed time and remaining account allowance.
- Browser calls use `/api/central/launchpad/<id>/trial`, with the existing Google-session and same-origin HMAC gateway. POST requires matching Origin. Anonymous clients cannot generate.
- The real directory adds only `trial_available`, not credential or consent-file contents.

The backend fixes model, generation settings, authentication and `Host` routing to the project-bound loopback FRPS ingress. Trials never use a public-pool fallback, so the preview tests the selected provider. Arbitrary URL/model/key/token-limit fields are rejected. Redirects and environment HTTP proxies are disabled. Errors do not echo upstream bodies or credentials. Responses retain only expected text/usage fields; key reflection is redacted and React renders response text without HTML execution.

Prompts go to the selected provider, which controls its own inference server and logging. The platform stores no prompt or answer text. `trial_requests` contains a hashed account/request identifier, project ID, time, outcome, elapsed time and reported token counts (7-day cleanup). `trial_quotas` stores counters and reset windows; `trial_leases` stores temporary concurrency ownership. Reported token usage is not proof of model weights or honest metering.

## Verification

The disposable Rust/MongoDB/Anvil integration exercises consent separation, non-owner trial use, revoked consent, finalized-project gating, parameter limits, idempotency, independent account quotas, HTTP 429 after two requests and shared project concurrency using a delayed inference stub. Gateway tests verify session/Origin requirements. Production smoke testing uses the actual hosted model through the assigned FRPC route; browser verification checks prompt input, real response, usage and remaining allowance. No contract transaction or credit purchase is part of these tests.
