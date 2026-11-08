# Contract cutover preparation

Status: **preparation only; legacy funds and runtime contract configuration remain active.** The modular core is locally tested, but its current version has no migration importer and the Rust/UI purchase/reservation flow is not yet connected. Do not deploy this core as the immutable migration target until these gaps are closed.

The read-only preparation UI/API was deployed to `https://riddle.xyz/#admin` at application revision `1cc8158d6ca13eb42f205423cde54aaa097e8fdb`. Health and unauthenticated rejection were checked; the browser showed the new readiness section. The live signed admin check requires the user’s wallet and was not performed by the agent. Chain and service configuration fingerprints remained unchanged.

## What is connected now

An admin who has authenticated with the configured wallet can run `POST /api/v1/admin/protocol` with their existing account-bound session token. The same-origin Node proxy exposes this as `/api/central/admin/protocol`. The admin page shows the active legacy stake address, staged candidates and unresolved cutover requirements.

This is read-only: there is no activate route, no database rewrite, no transaction preparation or broadcast. A successful audit proves the supplied, reviewed bytecode and fixed-peer bindings at a pinned finalized block; it does not prove that migration or application integration exists.

The endpoint requires Google session/HMAC gateway protection and the backend's admin-wallet session, rejects wrong-account/forged tokens, and keeps legacy registration, purchases, paid inference and finality tracking on the current contract configuration.

## Candidate manifest

Set `PROTOCOL_GROUPS_FILE` in the **Rust service environment**, separate from `CHAIN_CONFIG_FILE` and `CREDIT_CONFIG_FILE`. Omission means no staged candidate; it does not disable the existing service. An invalid manifest reports a blocked preparation state without switching contracts.

Start with [the empty example](../config/protocol-groups.example.json). A manifest has `schema_version: 1` and at most two entries under `candidates`. Each candidate requires:

- `id`: unique alphanumeric/hyphen ID, up to 64 characters;
- `version: "modular-v1"`, the existing RPC's `chain_id`, nonzero `deployment_block`;
- `prl_token`: the reviewed PRL address;
- `contracts`: distinct nonzero addresses for `factory`, `governance`, `registry`, `credit_token`, `usage_escrow`, `payment_escrow`, `stake_vault`, `sale_router`, `settlement_module`;
- `runtime_hashes`: reviewed Keccak256 runtime hashes for those same nine contracts. Hashes include deployed immutable values; they must come from a reviewed deployment/build, not be silently trusted from an arbitrary supplied address.

No RPC URL, signing key, `activate` flag or unknown property is accepted. The shared existing chain RPC performs the reads. Zero/duplicate/missing addresses, wrong chain/version and malformed hashes fail closed.

The checker pins a finalized block, checks deployed code, factory peers, registry/token/escrow/vault bindings, governance roles, initial active module roles and 48h delay, then verifies the checkpoint is still canonical. Individual candidate audits time out after 20 seconds. The initial module manifest must be revised/reviewed after a later authorized role rotation; a factory getter is not a live active-module registry.

## Remaining cutover gates

1. Finish a narrowly scoped importer/atomic coordinator before deploying the immutable target; preserve initial stake time, original issued supply and historical real delivery, and distinguish migration retirement from inference.
2. Implement per-project contract-group / ledger-epoch ownership in Rust and all monitors. Keep old groups readable and track their pending transactions independently.
3. Connect new wallet-authorized chain reservations and modular purchase/withdraw/settlement to the UI. V1's off-chain reservations must not be reused as if the new escrow already authorized them.
4. Exercise real Rust + MongoDB + browser + provider traffic for the new path, then fork and small Sepolia migrations; configure reviewed multisig governance.
5. Freeze the target project, wait for requests/burns/withdrawals, refresh the finalized all-holder inventory, obtain all holder consents and exact provider allowance/prefunding.
6. Review the concrete transaction manifest and irreversible boundary. Commit atomically, wait for canonical finality, reconcile, then switch the project's ledger epoch and resume service.

Replacing global configuration addresses, manually copying balances or invoking V1 full release alone is not a migration.

## Evidence for this preparation change

- Rust tests: candidate manifest validation and ABI address parsing.
- Gateway tests: same-origin authenticated POST, GET rejected and no activate endpoint.
- Real Rust/MongoDB/Anvil test: deploy a separate candidate, validate finalized runtime hashes/peers/governance, reject modified hash and swapped peer addresses, reject forged/wrong-account admin sessions, and confirm the legacy chain address remains active.
- Legacy paid purchase, real HTTP inference, burn and withdrawal regression continue in the same integration run.

These tests do not substitute for a new-contract migration rehearsal or an actual production cutover.
