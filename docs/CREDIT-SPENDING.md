# Buying and spending credits on launched models

Implemented test flow, 2026-11-08. Ethereum Sepolia only; use test ETH. Free prompt trials are separate and retain the 2/minute, 100/day account quotas. Existing PRL/tPRL stake remains locked: this release does not add proportional collateral release, slashing or pool fallback.

## What a buyer purchases

Each ERC-1155 token ID is the SHA-256 hash of one internal project UUID, tying the credits to its launch, model profile and provider. An authorized provider or offering manager creates an immutable supply, price in wei per credit, and model-ID hash after the original stake contract reports Verified. Sales can be closed. Verification alone does not create an offering.

In this test, **one credit pays for one reported completion/output token**. Input tokens are shown but not billed. The initial Qwen3 offer has 1,000,000 credits at 1,000,000,000 wei each: 0.001 Sepolia ETH per million credits. Network gas is additional. No production price or throughput guarantee is implied.

## Buyer steps

1. Sign in, choose a real launch and open **Buy & use test credits**. Read the model, provider, price and terms. The optional free preview uses the same actual provider API.
2. Connect a browser wallet. The app requests the configured chain and verifies the switch. Sign a five-minute, account/origin/chain/contract-bound EIP-191 challenge. The backend recovers the signer; each account is bound to one buyer wallet, and a wallet cannot bind to two accounts. Changing that binding is not supported in this test.
3. Confirm `buyAndLock(project, quantity)` in the wallet, paying the exact Sepolia ETH price. Credits mint directly into contract escrow and are credited to the buyer’s locked balance. There is no payment-token approval. The UI shows incoming credits, but the API accepts them only after Ethereum **finality**, not merely a mined receipt. This may take several minutes.
4. Submit a prompt in the launch’s paid page. The backend checks finalized stake/verification and provider consent, serializes requests for the wallet/launch and provider, and durably reserves the maximum requested output (16–256 tokens). It forwards only to the assigned FRPC ingress, injecting the private provider key. Buyers never receive that key.
5. Valid non-streaming generation consumes exactly the reported completion tokens. The remainder of the reservation becomes available. Known provider errors consume zero credits. The UI shows the response, usage and consumed amount awaiting settlement. Ordinary prompts need no wallet signature or transaction.
6. A dedicated platform settler periodically batches consumed requests, signs a transaction, persists its raw bytes/hash, then broadcasts. Retries rebroadcast the same transaction. The contract prevents reuse of a usage ID or changed reuse of a batch ID. A mined burn is pending until canonical Ethereum finality. Burned history links to the transaction.
7. The burn accrues the corresponding ETH to the offering’s provider. Only that provider wallet can call `claimRevenue()`. Unused purchase funds remain in the escrow to fund future consumption; providers cannot withdraw the entire sale proceeds upfront.

The app has a **My credits** page with separate balances per launch, rather than an interchangeable total across models/providers. The launch page refreshes balances every ten seconds while visible.

## Unused credits: withdrawal and redeposit

There is **no ETH refund** in this test. Withdrawal returns unconsumed ERC-1155 credit tokens to the buyer wallet, where they are not available to the inference API.

1. Buyer submits `requestWithdrawal(project, amount)`. New inference for that wallet/launch is fenced off as soon as the request is observed at latest or finalized state.
2. The settler waits for the request’s finality, no active reservations, and all recorded consumption to be burned and finalized. It submits `approveWithdrawal(project, buyer, nonce, amount)`.
3. Once approval is finalized, the buyer signs `completeWithdrawal(project)`. Alternatively, the buyer may sign `cancelWithdrawal(project)`; the nonce prevents stale approvals from authorizing a later request.
4. Buyer may sign `depositCredits(project, amount)` to return wallet tokens to usage escrow. No ERC-1155 operator allowance is needed for the owner’s own deposit. Deposits become usable after finality.

Tokens transferred to another wallet can be redeposited by their new owner. The platform supports one bound EOA per account; smart-contract wallet signature validation is not implemented. Withdrawal and revenue claiming still require wallet gas.

## What is recorded where

| Record/action | Location |
| --- | --- |
| Offering, immutable price/supply/model hash | New `ComputeCredits` contract |
| Purchase, deposit, withdrawal and per-buyer locked totals | Contract state/events |
| Prompt execution and maximum reservation | Rust service and durable MongoDB request metadata |
| Actual completion-token consumption | MongoDB immediately; on-chain after batch burn |
| Batch ID, usage-ID replay guards, burns and earned revenue | Contract state/events |
| Buyer wallet ownership proof | Signed off-chain challenge verified by backend |
| Prompt text | Sent to provider, not stored by the platform |
| Response text | Private account/wallet-bound MongoDB recovery cache for 15 minutes |
| Usage, request ID, burn transaction and settlement history | Durable metadata; not erased with response cache |
| Original PRL/tPRL stake | Existing stake contract, unchanged and locked |

Authoritative purchased/deposited/burned/withdrawn quantities are read from `accountState` at finalized and latest block tags. This release does not run a separate purchase-log indexer. Available usage is conservatively reconciled with all recorded consumption, active reservations, latest withdrawals and the current locked balance, so incoming deposits or pending withdrawals cannot manufacture availability.

## Failure and trust boundaries

Unique client request IDs bind to wallet + project. Replaying a completed request recovers the same answer for up to 15 minutes without generating or charging again. **Recover last request** is the safe action after a timeout. Unknown/interrupted in-flight requests remain reserved for operator reconciliation; they are not automatically billed or silently retried. Paid streaming and partial-response billing are not enabled.

MongoDB runs as a replica set with majority/journaled writes. Response-cache insertion and transition from reserved maximum to actual consumption commit in one transaction. Batch membership and queued actions also commit together. Signed transactions are saved before broadcast; the service resumes them after restart. Settlement errors remain visible and keep consumption unavailable. A chain-monitor halt blocks new purchase preparation/inference and settlement. Existing already-signed on-chain transactions and direct contract calls remain subject to their contract rules.

The platform settler is trusted to report usage correctly within escrow limits. The provider’s token counts and model ID are validated, but this is not cryptographic proof of GPU capacity, model weights or inference delivery. No public-pool replacement provider participates in this paid test. The contracts are test software, not audited production payment infrastructure.

## Contract transactions

| Actor | Call | When |
| --- | --- | --- |
| Provider / offer manager | `createOffering` | Once, after finalized project verification |
| Buyer | `buyAndLock` (payable) | Each purchase |
| Buyer | `depositCredits` | Returning wallet tokens to usage escrow |
| Dedicated settler | `settleBatch` | Periodic consumed-credit burns |
| Buyer | `requestWithdrawal` / `cancelWithdrawal` | Requesting or canceling an unused-token withdrawal |
| Dedicated settler | `approveWithdrawal` | After usage reconciliation and finality |
| Buyer | `completeWithdrawal` | After approval |
| Provider | `claimRevenue` | To collect ETH earned from settled output |

Original project registration, tPRL approval/stake and verifier `verifyProject` remain separate prerequisite transactions. Reading balances, signing wallet binding, sending prompts and recording reservations are not on-chain transactions.

## Operator configuration and tests

- `CREDIT_CONFIG_FILE`: JSON with `chain_id`, `contract`, `staking_contract`, `settler`, `deployment_block`; must match the configured stake chain. Deployment reference: `config/deployments/credits-sepolia.json`.
- `CREDIT_SETTLER_KEYSTORE` and `CREDIT_SETTLER_PASSWORD_FILE`: private encrypted dedicated settler wallet. `STAKE_CAST` and `CHAIN_RPC_URL` supply existing signing/RPC tools. Keep secrets outside Git and browser bundles. Fund this wallet with Sepolia gas; it must hold `SETTLER_ROLE` on the new contract. The worker checks stake-contract binding, role and signer address.
- `contracts/script/DeployCredits.s.sol`: Foundry deployment script, restricted to Sepolia/31337; separate from existing `LaunchStake` deployment. Always compare the chain RPC’s creation-gas estimate to Foundry’s local estimate before broadcast. The installed Foundry 1.5.1 underestimated this Sepolia deployment; the successful run used `--gas-estimate-multiplier 1000` after RPC simulation confirmed the higher requirement.
- Project owner must enable `paid_enabled` through the existing private RPC credential consent panel. An admin RPC-test credential alone does not permit paid use.
- `forge test --root contracts --match-contract ComputeCreditsTest` verifies contract accounting, purchase bounds, replay, withdrawals and earned revenue.
- `tests/stake_rust_e2e.py` exercises actual Rust + authenticated MongoDB + disposable Anvil, including buyer proof, purchase finality, inference, replay/recovery, automatic burns, withdrawal fencing, redeposit and revenue claiming.

Proportional PRL freeing remains the agreed future rule: `floor(initial stake × provider-delivered finalized-settled credits / committed supply)`. The existing stake contract cannot implement it, and no “free PRL” is invented from this credit ledger.

## Recorded live Sepolia test

[Live purchase and usage evidence](../config/test-results/paid-credit-sepolia.json): the dedicated test buyer purchased 1,000 credits, waited for finality, received `READY` from the hosted Qwen API, consumed 2 credits and retained 998 available. Replaying the request returned the same cached answer without charging again. The automatic burn transaction mined successfully; the snapshot explicitly records whether its finality had completed. The original 2,000 tPRL deposit was independently checked and remained locked. This live result is separate from the HTTP-fixture Anvil integration tests.

## Wallet transaction tracking

The backend persists wallet-submitted transaction hashes in `credit_wallet_transactions`, scoped to contract, project and bound buyer wallet. `POST /api/v1/credits/<project-uuid>/transactions` accepts only a hash. When the RPC can see it, the backend validates the sender, credit-contract destination, operation selector and project argument; a submitted hash never adds credits.

The browser stores an account/wallet/chain/contract-scoped fallback hash immediately after wallet submission and retries metadata registration on refresh, without resending the chain transaction. Successful purchases/deposits/withdrawal events are also recovered from chain logs, including older transactions that predate this tracking feature. Bounded historical scans use persistent cursors and always include recent activity. Previously unrecorded reverted or pending transactions cannot be discovered from successful event logs; their hash must be registered from the wallet.

Each row shows the hash/explorer link, action, amount, inclusion block, block confirmations, latest/finalized chain heads, and status: submitted, not yet found, mined awaiting finality, finalized, reverted, invalid or reorg reconciliation. Finality requires a successful receipt in the canonical block at or below the finalized head. RPC outages retain saved hashes with a status-unavailable warning. Balances remain based on finalized contract state, independent of this informational history. The history updates with the ten-second balance refresh while visible.

Ethereum finality is a consensus checkpoint rather than a fixed number of wallet confirmations. It normally trails inclusion by around 15 minutes, and can take longer; see [Ethereum’s finality explanation](https://ethereum.org/roadmap/single-slot-finality). The UI does not turn a block gap into a promised countdown or credit an unfinalized payment.
