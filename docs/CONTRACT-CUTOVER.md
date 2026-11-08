# Modular migration execution runbook

Reviewed 2026-11-08. Atomic migration and the Rust/API/React path are implemented and tested. Deployment of an inactive target does not change a project's current custody or ledger. Existing projects remain V1 until finalized migration and explicit activation.

## Recorded inactive Sepolia target

The new target is deployed; [deployment receipts and reviewed runtime hashes](../config/deployments/modular-sepolia-60s.json) are recorded. Group ID: `sepolia-modular-60s-20261108`. Governance: `0x79443baf29989e37d4f04fa1e4be58ec6d990732`; factory: `0x4188f96a18d89225c64ffc783997d3de6cb57e77`; gate: `0x38403bc9d001e7ecb1f92ef988bb6ed66fcc0221`. The gate is not installed, the candidate monitor is disabled, no legacy migration permissions have been granted, no source assets moved, and no governance delay has started. Wait for deployment finality before scheduling from Admin review.

The superseded 48-hour target’s first Timelock deployment reverted because the local Forge gas estimate was below the node requirement. Recovery regenerated fresh nonce/address simulation, used each node estimate with headroom, and persisted every transaction hash before waiting. All four successful deployments were checked against their expected addresses and compiled runtime artifacts. Future deployments must use node estimation for each creation; do not reuse the local script estimate blindly.

## Architecture and authorization

The stable contracts are ProjectRegistry, CreditToken, UsageEscrow, PaymentEscrow and StakeVault. SaleRouter and SettlementModule hold no user funds. Fixed peer references cannot be rebound; module role changes require the fixed per-chain LaunchpadTimelock delay: Sepolia 60 seconds, other chains 48 hours. The factory can install one fixed migration gate once, through that Timelock.

For Sepolia only, the user explicitly approved single-wallet proposer/executor `0x22b47B6689b34d88b9d259029561692cfD589442`. It is also the test verifier/guardian. This is not production multisig governance. The backend prepares fixed, reviewed transactions; governance, migration commitment and provider withdrawals require human browser-wallet confirmation. Backend admin actions verify fresh, account-bound wallet signatures.

`contracts/script/DeployModular.s.sol` deploys an inactive Timelock, reviewed ModuleDeployer, factory and fixed-project LegacyMigrationGate. It neither grants source-contract gate roles nor prefunds, releases, retires or activates anything. `scripts/pin_module_deployer_hash.py` checks the helper hash and EVM runtime/initcode limits; repinning is an explicit reviewed source/build change.

## Sequence

1. Deploy and review the inactive target, source/target peers, runtime hashes and roles. Stage it through `PROTOCOL_GROUPS_FILE`; preserve legacy chain/credit configuration. Set `monitor_enabled: false` while awaiting gate installation.
2. In Admin review, prepare **Schedule migration gate**. Confirm the fixed factory `configureMigration(gate)` Timelock operation in the approved wallet. The 60-second Sepolia delay starts when that transaction is mined, not when the backend prepares it. After readiness, execute that same operation and wait for finality.
3. Enable the candidate monitor before freezing. Check finalized group audit, code hashes, gate bindings and operator role. The manifest digest becomes the ledger identity once monitored; do not silently edit an active manifest.
4. Freeze the selected V1 project through the signed admin action. Drain inference, reserved requests, unsettled burns and pending withdrawals. Old service stays available for unrelated projects. Single-supervisor deployment must stop the old settlement worker before starting its replacement; mixed-version rolling workers are not supported.
5. Reconcile **every historical holder**, including fully consumed accounts. The UI roster is a hint, not authoritative completeness evidence. All issued credits must reconcile to escrow remaining plus real historical burns; external wallet token balances and outstanding withdrawals must be zero.
6. Grant the gate `SETTLER_ROLE` on V1 LaunchStake, plus `SETTLER_ROLE` and `OFFER_MANAGER_ROLE` on V1 ComputeCredits, only after review. Provider calls the gate’s payable `fund()` with the exact remaining ETH reserve and approves exactly the full PRL principal to the gate on the tPRL token. Only the provider may fund or recover funding through `withdrawFunding(uint256)`. Recompute amounts after freezing.
7. Create the short-lived plan in Admin review. Latest and finalized source snapshots must agree. Every holder reviews their own balance, destination contracts and plan hash, then signs EIP-712 `MigrationConsent(bytes32 planHash,uint64 expiresAt)`. Backend plans expire after ten minutes; the canonical checkpoint must remain within 256 blocks. Generating a new plan does not revoke earlier signatures on an unchanged plan before their expiry.
8. Prepare the atomic commit. The backend simulates the whole exact transaction with `eth_call`; review its fixed gate, calldata and zero ETH value. Submit from the authorized browser wallet. Any failed import, allowance, reserve or balance check reverts the entire transaction.
9. Wait for the successful canonical receipt to become finalized and for the new monitor to catch up. The signed activation action verifies the gate event, plan, imported terms, original stake time/deadline and target ledger before atomically selecting the project's contract group/epoch in MongoDB.
10. Verify balances, historical consumption, reserves, delivery, locked/free PRL and identity; perform a small new purchase, reservation, real generation, burn and provider withdrawal. Revoke obsolete source gate roles. Keep old receipts and addresses readable for audit.

## Atomic accounting

The gate first snapshots history. It closes V1 sales and retires remaining V1 credits, then releases the old full stake directly to its provider and pulls exactly that principal into the new StakeVault in the same transaction. Provider allowance is mandatory. It imports historical issuance, real prior delivery and each holder's remaining escrow/account history without double issuance.

V1 has no reserve-transfer migration function. The provider must prefund the target's remaining-credit ETH reserve. Source retirement makes the corresponding V1 reserve provider-receivable; the provider later claims that reimbursement separately. Migration retirement is explicitly tagged `MIGRATION_RETIREMENT` and never counted as AI delivery or new earned revenue in the target. It must not unlock extra PRL.

After migration, `Free = floor(initial stake × cumulative provider-settled credits / fixed committed supply) − already withdrawn`. Public-pool delivery, failures, unsettled requests and migration retirement do not count. The provider alone withdraws Free. Real inference remains a trusted platform-metering assertion; chain accounting cannot prove model weights or honest inference.

## Buyer flow after activation

The new purchase goes to SaleRouter and locks credits in UsageEscrow. The buyer signs a chain reservation for the maximum output budget, waits for finality, and invokes the assigned provider through the existing authenticated FRPC route. The backend records a durable request; a cached retry does not generate or charge again. Success queues the trusted settler transaction, which burns actual output credits and advances provider delivery. Known failure queues reservation release without a burn.

The current test policy remains one credit per output token, input free, 16–256 maximum output and one generation at a time per project. Unused locked credits can be withdrawn to ERC1155 wallet balance and redeposited; this is not an ETH refund. Pending wallet transactions and settlement requests are displayed separately from finalized spendable balances.

The migration adapter targets an existing launched project. Default new-project creation remains the legacy path until its own modular creation adapter is implemented. Public-pool failover, slashing, batched modular settlement and scoped session authorizations are not implemented.

## Cancellation and recovery

Before atomic commitment, provider funding is recoverable and allowance can be revoked. To resume V1 service, revoke all gate source roles, wait for latest and finalized evidence to agree and confirm the old stake remains verified; only then use the signed resume action. This prevents outstanding holder signatures from racing resumed spending.

After commitment there is no safe database-only rollback. Preserve receipts and reconcile forward; never replay V1 balances or point global addresses at the new contracts.

## Test evidence

- 60 Foundry tests (including Sepolia 60s, production 48h and early-execution/fixed-delay checks): migration completeness, consent/tampering/expiry, atomic rollback, issuance/reserve/delivery conservation and modular access/accounting.
- 19 Rust unit tests passed; one dedicated Mongo unit test ignored by the unit runner, with Mongo exercised by the integration suite.
- 54 frontend/gateway tests passed; TypeScript, server verification and production build checked.
- Full Rust + MongoDB + Anvil integration: local 48h governance scheduling/early rejection/execution, V1 freeze, holder signature, simulated atomic commit, finalized per-project activation, new purchase/reservation, authenticated HTTP inference, cached retry, durable burn and proportional PRL withdrawal. Legacy finality/reorg and owner/admin isolation regressions also passed.

Local HTTP inference uses an authenticated fixture; it is not a GPU capacity benchmark or external security audit. Actual Sepolia migration still requires live finality, holder consents and provider prerequisites.

Machine-readable local evidence: [modular-migration.json](../config/test-results/modular-migration.json).

### Sepolia test-delay correction

The original inactive 48-hour target is superseded by the 60-second test deployment. Its historical receipts remain recorded, but the backend candidate selects only the new target. Any schedule on the old Timelock does not configure the new target; use the new group shown in Admin review. No existing source assets are migrated by this replacement. Backend transaction preparation reads `getMinDelay()` from the deployed contract and validates the chain-specific policy. The UI no longer hard-codes a 48-hour schedule label.

## Migration UI feedback and transaction tracking

The project migration panel shows authorization challenge preparation, waiting for the wallet message signature, server verification and the separate wallet transaction confirmation. Signing a message is never displayed as successful transaction submission. Errors and rejected wallet requests remain beside the controls instead of only at the top of the page.

An authenticated, same-origin `POST /api/central/admin/migration-status` polls current chain state without another wallet signature. The backend derives the fixed operation ID and reads the Timelock timestamp, latest/finalized gate binding and chain time. The UI displays not scheduled, governance countdown, ready to execute, binding mined awaiting finality, and finalized binding. The countdown is approximate between ten-second polls; only a refreshed chain result enables Execute. A submitted transaction disables duplicate submission.

Returned transaction hashes are retained locally and registered with the backend. The backend validates the transaction sender against the configured admin and its destination against the selected governance/gate before saving it in MongoDB `migration_transactions`. Canonical receipts distinguish submitted, mined awaiting finality, finalized, reverted and reorged states. Hashes link to the explorer and show inclusion blocks alongside the finalized head. Local tracking failures retain the hash for retry; they do not imply transaction failure or authorize a replacement. Earlier locally saved governance hashes are recovered when they match the selected target; stale hints can be dismissed without deleting server history.

This endpoint is read-only apart from validated transaction metadata. It cannot schedule governance, move assets or activate a ledger. Existing fresh wallet-signature checks still protect every consequential admin action.
