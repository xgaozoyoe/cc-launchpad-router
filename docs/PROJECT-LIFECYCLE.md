# Stake-gated project launch

See [implementation status](IMPLEMENTATION-STATUS.md) for the current scope. The active implementation uses Rust/MongoDB, not the Python/SQLite reference monitor.

## Active production flow

1. Create an account-owned model-specific draft. Only the internal project UUID exists.
2. Bind the provider wallet with a project-bound EIP-191 signature. The server registers its SHA-256 project key, wallet and fixed stake amount on Sepolia.
3. The provider approves test tPRL and stakes. Submission/mining alone does not issue an ID.
4. The Rust worker verifies the finalized canonical stake event and atomically reserves the Launch ID. Provider ID is the full normalized stake transaction hash; one direct stake transaction backs one project.
5. The project waits in `awaiting-rpc`. The deadline is `stake block timestamp + 86400`, including time spent waiting for finality. At/after the deadline it cannot pass acceptance. Expiry uses the latest chain block timestamp.

My launches lists the owner's server projects, supports stage filters/search and resumes staking or displays deployment requirements. Drafts do not receive IDs. Browser-supplied flags, IDs and timestamps cannot confirm stakes. See [Rust staking](RUST-STAKING.md) for validation, configuration, failure recovery and tests.

## Remaining publishing work

Private project deployment credentials, owner-only Linux bundles and FRPS authorization against MongoDB/chain state are integrated in [provider bundles](PROVIDER-BUNDLES.md). Functional RPC/model/usage checks and [wallet-authorized manual demo approval plus on-chain acceptance](ADMIN-REVIEW.md) are integrated. Measured capacity benchmarks and sale activation remain unintegrated. No live sales can be opened through the current Rust API. The target is mandatory FRPC, then successful acceptance within the original 24h window, then sales on the same Launch ID.

The contract includes trusted `verifyProject` and full settlement/refund operations; their presence does not mean the application has automated them. A provider may call `refundExpired` for an unverified expired stake. Expiry alone does not transfer funds or slash collateral.

## Collateral after delivery

Initially all deposited PRL is locked. The agreed future rule is:

`cumulative unlocked = initial stake × provider-delivered and settled credits / fixed committed credit supply`

Only that provider's successful, settled delivery counts. Failed requests, unsettled usage and public-pool replacement do not count. The cumulative amount must be capped at the initial stake, use raw token arithmetic with rounding down, and be idempotent across repeated settlements. Free means unlocked but not yet withdrawn; returned means already transferred back. These settlement rules and partial withdrawal are **not implemented** in the current contract. Its full release/refund transfers the stake directly to the wallet.

## Separate reference implementation

`frp/server/lifecycle.py`, `registry.py`, `router.py`, `monitor.py` and the Python stake CLI provide separate transport/lifecycle examples with SQLite and a private bundle outbox. They are not the production project writer. Their credential export and acceptance methods are not HTTP endpoints of the active Rust service. Never run the reference monitor as a second writer for the active deployment.

`frp_admin.py add` and `launch` are disabled. `init` and `list` remain usable for the separate FRP infrastructure. Generic transport code supports more flexible provider/launch associations; the active workflow uses a project stake hash as Provider ID. Long transaction hashes need the deterministic internal alias enforced by that reference registry. A connected tunnel does not prove capacity or exclusive hardware ownership.

The browser-local `ProjectLaunch` fallback remains a labeled simulation for standalone demo mode; it is not the integrated production flow.

## After verification: paid model usage (proposed)

See [Spending credits on launched models](CREDIT-SPENDING.md) for purchase and locking, request reservations, usage accounting, on-chain batch burns, withdrawal reconciliation and proportional collateral release. Normal prompts do not require individual on-chain transactions. These paid-credit steps are not implemented; the existing free prompt preview is separate.
