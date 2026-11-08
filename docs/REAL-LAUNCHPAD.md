# Real Launchpad and isolated Demo workspace

The default authenticated app reads the central backend. It no longer shows seeded providers, illustrative credit prices, simulated balances, fake network activity or public-pool capacity. The signed-out landing page describes the real Sepolia testnet features and explicitly says credit sales are not yet enabled.

## Real directory

`GET /api/v1/launchpad` (browser: `GET /api/central/launchpad`) requires the normal Google-session/HMAC gateway. It lists up to the latest 100 projects with a finalized stake, reserved Launch ID, matching configured chain/contract and status `awaiting-rpc`, `verification-pending`, `accepted` or `chain-halted`. Private drafts, pending stakes and refunded/released/expired records are excluded. `has_more` indicates truncation.

The endpoint projects an explicit public field allowlist. It returns project/Launch/Provider IDs, name, model, status, test-token stake commitment and sanitized historical RPC progress. A capacity decision is reduced to a boolean; its actor, account and reason are removed. No wallet challenge, email, account identity, private connector bundle, API key or signing material is returned. The directory is shared among signed-in users; account-specific My launches retains its existing ownership checks.

The frontend shows real project counts and finalized verification counts from the returned records, not global estimates. API results show the original check time and are not continuous health claims. Errors remain visible; no sample listings replace unavailable backend data. The directory refreshes every 30 seconds while visible.

Credit pricing, issuance supply, payment, a real buyer ledger and redemption remain unfinished. The directory returns `sales_enabled:false`, shows price/supply as unavailable and disables purchasing. My credits says the real ledger is not connected; it does not display a made-up zero or simulated balance. The registered model list comes from the backend's Qwen3 8B AWQ serving profile.

## Provider management

Create project, My launches, provider setup and Admin review use the actual backend. Project creation does not fall back to local simulation when the backend is unavailable. The provider guide points to actual project-bound FRPC and RPC setup. PRL escrow details remain visible in the owner's project; there is no default simulated PRL wallet or pool dashboard.

## Demo isolation

`/#demo` is an explicit, visibly labelled simulation workspace with the prior illustrative offers, purchases, balances, public-pool behavior and model catalog. It keeps the existing per-account browser storage so old demo progress remains available. Its project dialog uses only the local simulation reducer: it does not create central projects, sign wallet messages or send transactions. Returning to the real Launchpad does not carry those credits or balances into the backend. Legacy `#prl` and `#pool` links resolve to Demo; `#resources` now opens the real provider guide.

The default app does not initialize or display the simulation reducer. Demo state is loaded only when the Demo workspace is mounted. Seeded models/providers are not claims of platform support or live offers.

## Verification

The Rust/MongoDB/Anvil integration test checks that another signed-in user can read listed launch metadata but not owner project details, that drafts and inactive projects are excluded, and that approvals and credentials do not leak into directory output. Gateway tests cover authenticated GET and rejection of writes. Browser verification checks the actual deployed project, disabled purchase state, real supported model list and the separate Demo navigation.
