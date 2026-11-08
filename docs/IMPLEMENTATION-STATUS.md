# Implementation status

Reviewed 2026-11-08 against repository code, the committed Sepolia deployment/test manifests and the native Ubuntu deployment layout. This is a capability inventory, not a promise of continuously healthy services. Test reports record past results; project status and deadlines can change.

## Planned contract architecture (not deployed)

The next architecture separates stable PRL custody, payment reserves, credit ownership and usage accounting from replaceable sale/settlement modules. See [architecture decision](CONTRACT-ARCHITECTURE.zh-CN.md). The progressive V2 prototype is a reference only, not the migration deployment target. Existing production contracts and funds remain unchanged; migration requires the new implementation, tests and a revised execution plan.

## Active application

The integrated origin is **https://riddle.xyz**. Caddy terminates HTTPS, Node serves React/Auth.js on tailnet port 9001, Rust runs on loopback 9002, and a dedicated authenticated MongoDB replica set uses loopback 27189. Vercel configuration is retained as an alternative, not the active central backend. The public inference route examples using `api.example.com` are placeholders.

| Capability | Actual implementation |
| --- | --- |
| Google sign-in | Connected; stable Google subject, signed-session legacy linking and account isolation |
| Model-specific project creation/list/details | Rust + MongoDB; authenticated through same-origin HMAC session gateway |
| My launches | Real owner records, filters/search, resume/detail, stake transaction, Launch/Provider IDs, RPC deadline and green/yellow/red progress checklist joined from saved connector/RPC records |
| Wallet proof and stake | EIP-191 EOA binding, fixed operator registration, provider approval/stake on Sepolia |
| Event monitor | Rust worker reads finalized logs, verifies evidence, atomically reserves IDs, checkpoints and halts on finalized-checkpoint changes |
| 24h window | Stake block timestamp + 86400; confirmation consumes part of the window; expiry uses chain time |
| Launch ID | Issued after accepted finalized stake; not permission to sell |
| Provider setup guide | Opens from My launches; project-bound instructions/checklist and saved status refresh. Owner-only Linux FRPC archive download; historical authenticated login/proxy timestamps. Saved functional Test RPC results; no live heartbeat or formal capacity acceptance |
| Collateral display | Locked/free/returned derived from recorded project state; not a wallet balance or a partial unlock ledger |
| Unverified expired refund | Contract supports provider `refundExpired`; no automated refund or UI withdrawal flow |
| Full verified release | Contract supports trusted settler `releaseStake`; application settlement is not integrated |
| Default project directory and provider setup | Real backend records and model registry; no seeded offers or simulated balances |
| Wallet transaction tracking | Persistent submitted hashes, validated receipts, historical purchase recovery, pending/finalized/reverted status and current chain heads in My credits |
| Real buyer purchasing and usage | New test credit escrow, wallet proof, finalized balances, actual API inference and automatic batch burns; existing PRL stays locked |
| Buyer offers, credits, PRL wallet, inference and pool in `/#demo` | Explicit concept simulations/browser-local data, not production token balances or measured GPU capacity |

Publishing can advance from `awaiting-rpc` through signed manual demo capacity approval and `verification-pending` to finalized on-chain `accepted`. See [admin review](ADMIN-REVIEW.md). This is not a measured capacity benchmark. A separate test-only `ComputeCredits` contract adds fixed offers, Sepolia ETH purchase/escrow, wallet-bound buyer balances, paid inference reservations, batch burns and earned provider revenue. Offers require separate creation and provider paid-inference consent. Slashing, proportional PRL release and public-pool failover remain unimplemented. See [credit spending](CREDIT-SPENDING.md).

## Contracts and recorded evidence

Ethereum Sepolia chain ID: 11155111. The token is test-only tPRL, not real PRL.

- tPRL: `0x3347f108f03287bdb39cba95f40f7f39e6dff1f8`
- LaunchStake: `0xe5b29f59c9c90b7fbde165273a3464a35875c58c`
- ComputeCredits: `0x4c18665f28ac99745edda9b62cf3668ff0f62faf`
- [Credit deployment and roles](../config/deployments/credits-sepolia.json) · [Initial Qwen test offering](../config/deployments/credit-offer-sepolia.json)
- [Stake deployment manifest](../config/deployments/sepolia.json)
- [Recorded 2,000 tPRL stake and finalized Launch ID](../config/test-results/sepolia-stake.json)

A stake is collateral and an eligibility requirement; possession of tPRL alone does not prove GPU ownership, model correctness or exclusive available capacity. Contract verification is a trusted verifier action. No production PRL token integration or external contract audit is claimed.

## Functional RPC checks

The owner can run [Test RPC](RPC-CHECKS.md) through the assigned tunnel using a one-time inference key. Health/authentication, advertised model ID, generation, usage arithmetic and SSE are checked, and sanitized results are saved. Passing does not verify underlying weights/capacity, call the contract verifier or enable sales.

## Agreed product rule, not implemented

Initially lock all deposited collateral. Cumulatively unlock in proportion to credits successfully delivered **by that provider** and settled, divided by a fixed issuance commitment. Failed/unsettled requests and public-pool replacement do not count. Free is unlocked but unwithdrawn; returned is already transferred. See [lifecycle accounting](PROJECT-LIFECYCLE.md#collateral-after-delivery).

The existing contract transfers the whole deposit on release/refund; it cannot implement proportional unlocking or separate withdrawal. A future contract and trusted settlement/metering path must add immutable committed supply, cumulative settled delivery, idempotency and token accounting. Do not calculate withdrawable funds from the demo reducer or announce that gradual release is live.

## Separate reference stack

The Python FRP stack implements transport authorization, route ownership, launch routing and a separate SQLite lifecycle/monitor with private bundle export. It is not the production Rust/MongoDB writer. The native Rust service now provides owner-only FRPC bundles and project authorization on the existing dedicated FRPS; see [provider bundles](PROVIDER-BUNDLES.md). This does not synchronize the legacy Python registry or activate buyer routes. Manual `frp_admin.py add`/`launch` commands are disabled. `init` starts an empty FRP registry; it does not provision Rust-issued IDs. FRPC remains mandatory in the intended deployment architecture.

Real local FRP transport was tested against mock inference. GPU model loading and the FRP Docker/public inference deployment are unverified. Native HTTPS, Google login and finalized Sepolia staking were verified separately; they do not establish working inference sales.

## Documentation map

- [Root overview](../README.md), [UI](../apps/launchpad/README.md), [central backend](../central/README.md)
- [Active Rust staking](RUST-STAKING.md), [lifecycle and agreed rules](PROJECT-LIFECYCLE.md)
- [Contract and Python monitor reference](STAKING-CONTRACT.md), [Forge deployment](../contracts/DEPLOYMENT.md)
- [Supplier transport](HOSTING.md), [separate FRPS operator stack](FRP-HOSTING.md)
- [Native Ubuntu operations](../deploy/remote/README.md), [local/Compose alternative](../deploy/stack/README.md)
- [Google callback setup](../apps/launchpad/GOOGLE-SIGN-IN-SETUP.md), [中文说明](README.zh-CN.md)
- [White paper review](WHITEPAPER-STATUS.md)

When adding a feature, update its implementation, tests and this inventory together. Keep private deployment configuration, keys and logs out of public documentation.

The default UI now uses the authenticated [real launch directory](REAL-LAUNCHPAD.md), with public metadata from finalized-stake projects and actual verification status. Private drafts and account/configuration fields are excluded. The old seeded offers, credit balances and public-pool UI are isolated under `/#demo`. My credits now reads per-launch chain escrow balances and backend reservations. Buy & use supports test ETH purchases, bounded real inference, burn history and reconciled unused-token withdrawals.

The implemented test paid-credit flow and its transaction/state boundaries are specified in [Spending credits on launched models](CREDIT-SPENDING.md). Existing PRL stays locked; proportional collateral unlocking remains future work.
