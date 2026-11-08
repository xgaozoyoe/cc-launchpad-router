# Modular migration execution runbook

Reviewed 2026-11-08. Atomic migration and the Rust/API/React path are implemented and tested. Deployment of an inactive target does not change a project's current custody or ledger. Existing projects remain V1 until finalized migration and explicit activation.

## Architecture and authorization

The stable contracts are ProjectRegistry, CreditToken, UsageEscrow, PaymentEscrow and StakeVault. SaleRouter and SettlementModule hold no user funds. Fixed peer references cannot be rebound; module role changes require the 48-hour LaunchpadTimelock. The factory can install one fixed migration gate once, through that Timelock.

For Sepolia only, the user explicitly approved single-wallet proposer/executor `0x22b47B6689b34d88b9d259029561692cfD589442`. It is also the test verifier/guardian. This is not production multisig governance. The backend prepares fixed, reviewed transactions; governance, migration commitment and provider withdrawals require human browser-wallet confirmation. Backend admin actions verify fresh, account-bound wallet signatures.

`contracts/script/DeployModular.s.sol` deploys an inactive Timelock, reviewed ModuleDeployer, factory and fixed-project LegacyMigrationGate. It neither grants source-contract gate roles nor prefunds, releases, retires or activates anything. `scripts/pin_module_deployer_hash.py` checks the helper hash and EVM runtime/initcode limits; repinning is an explicit reviewed source/build change.

## Sequence

1. Deploy and review the inactive target, source/target peers, runtime hashes and roles. Stage it through `PROTOCOL_GROUPS_FILE`; preserve legacy chain/credit configuration. Set `monitor_enabled: false` while awaiting gate installation.
2. In Admin review, prepare **Schedule migration gate**. Confirm the fixed factory `configureMigration(gate)` Timelock operation in the approved wallet. The 48-hour delay starts when that transaction is mined, not when the backend prepares it. After readiness, execute that same operation and wait for finality.
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

- 58 Foundry tests: migration completeness, consent/tampering/expiry, atomic rollback, issuance/reserve/delivery conservation and modular access/accounting.
- 19 Rust unit tests passed; one dedicated Mongo unit test ignored by the unit runner, with Mongo exercised by the integration suite.
- 54 frontend/gateway tests passed; TypeScript, server verification and production build checked.
- Full Rust + MongoDB + Anvil integration: 48h governance scheduling/early rejection/execution, V1 freeze, holder signature, simulated atomic commit, finalized per-project activation, new purchase/reservation, authenticated HTTP inference, cached retry, durable burn and proportional PRL withdrawal. Legacy finality/reorg and owner/admin isolation regressions also passed.

Local HTTP inference uses an authenticated fixture; it is not a GPU capacity benchmark or external security audit. Actual Sepolia migration still requires live finality, holder consents and provider prerequisites.

Machine-readable local evidence: [modular-migration.json](../config/test-results/modular-migration.json).
