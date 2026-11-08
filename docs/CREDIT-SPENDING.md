# Spending credits on launched models

Status: proposed design, documented 2026-11-08. This document specifies the intended paid purchase-to-inference flow. It does not describe an implemented credit contract, paid API or production settlement service. Existing real prompt trials remain a separate free, rate-limited preview and do not purchase, reserve or burn credits.

## Credit identity and unit

Credits belong to a specific launch and its model/serving profile. Credits from different launches are not interchangeable, even when they advertise the same model. The offering must fix its committed supply, price, delivery terms and metering unit before purchase. The current concept uses output-token credits; the treatment of input tokens and any weighted pricing must be explicitly defined before implementation, not inferred from a provider's API response.

## Buyer journey

1. Select a verified launch, read its terms and optionally try the real model through the free preview.
2. Connect and bind the buyer wallet to the authenticated account. A wallet ownership signature is off-chain and does not itself purchase or transfer tokens.
3. Purchase credits through the offering contract. The backend recognizes purchased credit balances only after validating canonical, finalized purchase events; a browser claim or transaction hash alone is insufficient.
4. Lock credits in a usage contract. Purchase and locking may be combined into a single transaction. Otherwise a separate deposit transaction is required. Freely transferable wallet tokens cannot also be treated as spendable API balances: they must first be placed under the usage contract's spending rules.
5. Submit a prompt through the platform's launch-specific API. The backend atomically reserves the maximum billable credits before forwarding to the assigned provider. Normal prompts do not require wallet confirmation or a separate blockchain transaction.
6. On successful delivery, record billable usage, mark the actual amount consumed awaiting settlement, and release unused reservation. On a failed request with no billable output, release the reservation without consuming credits. Interrupted streams and partial delivery require an explicit billable-delivery policy before streaming is enabled.
7. A platform-authorized settler periodically submits an on-chain batch burn against locked credits. Track each settlement until finalized, then move the consumed amount into burned history. Do not count a submitted or reverted burn as finalized spending.
8. Withdraw unused locked credits only after reconciling active reservations and unsettled usage. A withdrawal request must fence off the requested amount from new inference and settle outstanding consumption before contract withdrawal is allowed. Immediate withdrawal of the entire on-chain deposit would otherwise allow double spending against an off-chain reservation.

## Balance states

| State | Meaning |
| --- | --- |
| Wallet credits | Held in the buyer's wallet; not yet available to the API |
| Available | Finalized locked credits not reserved, consumed or pending withdrawal |
| Reserved | Temporarily allocated to an in-progress inference request |
| Consumed, awaiting settlement | Billable delivery recorded in the backend; on-chain burn not finalized |
| Burned | Consumption settled and finalized on-chain |
| Pending withdrawal | Unspent credits fenced from new usage while withdrawal is reconciled |

These must be separate balances in the UI. Backend records should reconcile finalized deposits, burns and withdrawals with reservations and unsettled consumption. Off-chain consumption is immediate accounting; it is not itself an on-chain burn.

## On-chain transactions

| Actor | Transaction | Frequency/condition |
| --- | --- | --- |
| Platform operator | Register the provider project | Current staking setup |
| Provider | ERC-20 PRL allowance, then project stake | Allowance only if insufficient; stake once per project |
| Authorized verifier | Verify the project | After acceptance checks, before the original stake-based deadline |
| Provider | Create the credit offering with fixed supply and terms | Before opening sales |
| Buyer | Approve an ERC-20 payment token | Only when paying with an ERC-20 and allowance is insufficient |
| Buyer | Purchase credits | Each purchase; native ETH payment avoids ERC-20 payment approval |
| Buyer | Approve/deposit credits into usage escrow | Token-standard dependent; may be combined with purchase |
| Authorized settler | Burn consumed locked credits in a batch | Periodic settlement; never burn arbitrary unlocked wallet tokens |
| Provider | Withdraw collateral made free by finalized settlement | When a withdrawable balance exists |
| Buyer | Withdraw reconciled unused credits | After pending usage/withdrawal checks |

EOA binding signatures, prompts, reservations and MongoDB usage entries are not blockchain transactions. Reading balances or checking transaction status is also not an on-chain transaction. Native Sepolia ETH is a suggested initial test-payment option, not an implemented or finalized production payment choice. Credit token standard, escrow/withdrawal protocol, payment custody and supplier payment timing still require contract design.

## Backend accounting and settlement integrity

Proposed records include offerings, wallet/account bindings, finalized purchases/deposits, per-launch user balances, inference requests/reservations, usage records, settlement batches/items, finalized burns, withdrawals and an event-indexing checkpoint. None of these paid-credit ledgers should be confused with the existing free-trial quota collections.

Use durable unique request IDs and atomic balance reservations. A retried request must not generate or charge twice. Associate usage with the actual delivering provider and launch; reported API token counts need a defined metering/trust policy. Persist a batch identifier and item membership before broadcasting settlement, prevent an item from entering multiple batches, and make contract settlement idempotent. A crash, RPC timeout or transaction replacement must trigger reconciliation of the existing transaction/batch rather than a blind second charge.

The backend and chain indexer must handle failed transactions, finality and reorgs. Unfinalized incoming credits must not become spendable; submitted burns must not release collateral. Prompts and outputs should not be put on-chain. Paid prompt/output retention and provider privacy terms must be defined separately from required usage/accounting metadata.

## Provider collateral and fallback

Initially lock all stake. Under the agreed target rule:

`cumulative unlocked PRL = floor(initial stake × this provider's successfully delivered and settled credits / fixed committed credit supply)`

Cap cumulative unlocked PRL at the initial stake. Free collateral is unlocked but not yet withdrawn; returned collateral has already been transferred to the provider wallet. Only finalized, eligible settlement can increase free collateral. Failed requests, unsettled usage and public-pool replacement delivery do not count toward the original provider's fulfillment. Record fallback delivery separately and settle it under an explicit fallback funding/billing policy; do not charge the same request twice.

The authorized settler is trusted to submit correct usage accounting within contract limits. Locking prevents wallet-transfer double spending but does not cryptographically prove that inference happened. Settlement evidence/disputes, role limits, withdrawal fencing, pricing/metering and payment release require explicit design before enabling paid sales.

## Current implementation boundary

Implemented: project registration, test tPRL approval/stake, finalized stake monitoring, project-bound FRPC/API checks, admin verification, real project directory and limited free prompt trials.

Not implemented: paid credit offering/issuance, payment, buyer credit escrow/ledger, paid reservations, credit burns, batch settlement, buyer withdrawals, proportional collateral unlocking, provider revenue settlement or public-pool delivery billing. The deployed stake contract currently supports whole-stake release/refund rather than the proposed partial unlock/withdraw mechanism. Purchasing must stay disabled until the paid flow and its contract rules are implemented and tested.
