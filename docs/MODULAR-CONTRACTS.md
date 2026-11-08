# Modular contract core — development reference

Status: contracts, atomic migration, Rust settlement/monitor and UI implemented and locally integrated. **Existing funds remain on V1 until an explicit finalized migration.** See [execution runbook](CONTRACT-CUTOVER.md) for deployment versus activation boundaries. This replaces the earlier combined V2 prototype as the architecture direction. See [architecture and trust boundaries](CONTRACT-ARCHITECTURE.zh-CN.md) and [test evidence](../config/test-results/modular-protocol.json).

## Code and ownership

All new contracts are under `contracts/src/modular/`; V1 and the earlier progressive prototype are preserved unchanged.

| Contract | Holds / records | Authorized entry |
| --- | --- | --- |
| ProjectRegistry | Immutable project economic terms and lifecycle | Registrar, verifier; fixed StakeVault records staking/refund |
| CreditToken | ERC1155 balances, lifetime issued cap | Fixed PaymentEscrow issues; fixed UsageEscrow burns custody |
| UsageEscrow | Locked credits, reservations, receipts, buyer nonces, provider delivery | Buyer or EIP-712 buyer authorization; authorized settlement module |
| PaymentEscrow | ETH reserves and earned provider revenue per project | SaleRouter purchases; fixed UsageEscrow settles; provider claims |
| StakeVault | PRL principal, withdrawals, proportional Free | Provider stakes/withdraws; reads fixed UsageEscrow delivery |
| SaleRouter | No retained funds or balances | Buyer transaction forwards all ETH |
| SettlementModule | No retained funds or balances | Trusted operator submits provider receipt or known failure |
| LaunchpadTimelock / GovernedAccess | Fixed Sepolia 60s / other chains 48h governance / pinned role administrator | Reviewed proposer/executor, expected to be multisig for deployment |
| TestnetProtocolFactory | Sealed address manifest, no admin left | Constructor bootstrap plus one-time timelocked migration-gate binding; chain 31337 or 11155111 |

The factory accepts an already deployed LaunchpadTimelock and PRL token, then deploys all peers, binds them once and seals role administration. A factory getter continues to identify the **initial** router/module after rotation; current authorization is determined by stable-contract role events, not by treating the initial getter as an active-module registry. `DeployModular.s.sol` deploys an inactive testnet target. It grants no legacy migration roles and moves no legacy assets. `LegacyMigrationGate` imports one fixed project atomically after timelocked installation, exact provider allowance/prefund and every historical holder’s consent.

## Local verification

From the repository root (Foundry and the existing Python integration environment required):

```sh
cd contracts
forge test -vv
cd ..
/tmp/cc-central-venv/bin/python tests/modular_protocol_e2e.py
```

The Python path above is this workspace's environment; elsewhere install `eth-abi` and `eth-utils` in your own venv. The test starts a private disposable Anvil node and an authenticated HTTP provider fixture, deploys fresh contracts, purchases/consumes/withdraws and terminates both processes. It never uses production RPC, deployment keys or database configuration. The fixed model fixture does not measure GPU capacity or validate actual model quality.

The Foundry suite covers role rotation through a 48h timelock, cross-project reserves, signature replay/domain separation, purchase rollback, partial/full PRL withdrawals, failed/expired requests, credit transfers and revenue reentrancy. New-contract runtime and factory initcode were checked against EVM size limits; the local constructor deployment consumed 8,159,967 gas.

## Call flow to integrate next

1. Register fixed terms; provider approves PRL to StakeVault and calls `stake(project)`. Registry timestamps the actual staking transaction. Verifier calls `verifyProject(project)` before that timestamp +24h.
2. Buyer calls `SaleRouter.buyAndLock(project, amount)` with exact ETH value. PaymentEscrow records reserve and calls CreditToken to mint directly into UsageEscrow, crediting the buyer atomically.
3. Buyer calls `UsageEscrow.reserve(project, reservationId, maximum, expiresAt)`, or signs the EIP-712 `Reservation` payload and a relayer calls `reserveWithSignature`. Maximum lifetime is 24h. Nonce, project, chain and escrow are bound; there is no unlimited server spending approval.
4. Backend waits for the appropriate reservation confirmation policy before serving. Trusted SettlementModule submits `settleProvider(reservationId, usageId, actual)`, bounded by the reserved maximum. The entire reservation closes, only actual credits burn, unused maximum becomes available again. Settlement atomically moves the matching project ETH from reserve to earned and increments provider delivery.
5. Known failures use `releaseFailed`; anyone can call `releaseExpired` after expiry, and a late settlement then fails. An expired delivered-but-unsettled request cannot be charged later: the backend must manage timeouts/settlement delivery before the deadline. This is an explicit provider risk, not proof that all network failures are free of ambiguity.
6. Provider reads `StakeVault.collateralState(project)` and personally calls `withdraw(project, amount)` up to Free. Revenue is separately claimed from PaymentEscrow.
7. Buyers can call `UsageEscrow.withdraw` only for unreserved tokens, transfer wallet tokens and deposit them using ERC1155 `safeTransferFrom` to UsageEscrow. Reservation state is already on-chain, so this differs from V1's server-reconciled withdrawal approval. **Do not reuse the V1 off-chain reservation service unchanged.**

Normal generation currently requires a chain reservation transaction (buyer or relayer), followed by settlement; batched settlement and scoped session authorizations are not implemented. Migration and fallback receipts have no ordinary provider-settlement entry, so callers cannot label a migration burn or pool delivery as provider fulfillment through a supported API. The trusted operator can still lie about real inference; chain accounting alone does not remove that trust.

## Before migration

Implement contract-group/ledger-epoch routing, finalized event indexing, frontend signed reservation/purchase/withdraw flows, scoped import and migration coordinator, and fork/Sepolia rehearsals. Preserve buyer consent and prefunded ETH-reserve requirements from the migration review. No migration target is ready for broadcast yet.
