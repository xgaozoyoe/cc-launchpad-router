# Stake contract and confirmed-event Launch ID issuance

This repository includes an **EVM / ERC-20 implementation**. The development default chain is **Ethereum Sepolia (11155111 / 0xaa36a7)**, defined in `config/network.json`. Configure the Sepolia RPC, test PRL token address and deployed `LaunchStake` address before running the monitor. A test-only tPRL and LaunchStake deployment is recorded in `config/deployments/sepolia.json`; the active Rust/MongoDB staking implementation is documented in [RUST-STAKING.md](RUST-STAKING.md). The Python monitor described below is a separate reference implementation. Non-EVM chains need a separate adapter.

## Contract flow

1. The authenticated backend creates a draft project with wallet owner, exact model profile and required stake in **raw PRL units**.
2. The platform admin registers its key, wallet and exact amount in `LaunchStake.registerProject`.
3. The wallet approves the PRL token transfer and calls `stake(projectKey)`.
4. `stake` transfers the exact amount into escrow and emits:

```solidity
event ProjectStaked(
    bytes32 indexed projectId,
    address indexed provider,
    uint256 amount,
    uint64 stakedAt,
    uint64 verificationDeadline
);
```

`stakedAt` comes from `block.timestamp`, and the deadline is exactly `stakedAt + 24 hours`.

5. The monitor waits for a **finalized** canonical stake event, verifies it and assigns the Launch ID and deployment credential. Provider ID equals the complete stake transaction hash.
6. A trusted verifier performs RPC/model/usage/capacity checks and submits `verifyProject` to the contract **before the deadline**. After that transaction is finalized, the authenticated backend may call `Monitor.accept_rpc` with its trusted probe to mark the registry launch live.

Creating a Launch ID is not permission to sell. The public router rejects pending launches. The monitor never signs or broadcasts verifier/admin transactions.

## Permissions and custody

- Admin: registers projects and grants/revokes verifier/settler roles. Constructor grants only admin; verifier and settler roles must be assigned explicitly.
- Provider: stakes only its own registered project and exact amount. Duplicate stakes and unknown projects are rejected.
- Verifier: can accept pending projects only before the deadline. It is a trusted off-chain oracle, not cryptographic proof of GPU performance.
- Provider can reclaim unverified stake at/after the deadline with `refundExpired`. Verified stake cannot use this path.
- Settler: can release verified stake with an auditable nonzero reason after delivery/disputes. This is a trusted settlement decision; automatic delivery proof and slashing are not implemented.

The contract uses OpenZeppelin 5.4.0 `SafeERC20`, `AccessControl` and `ReentrancyGuard`. Fee-on-transfer deposits are rejected by checking the actual received balance. Use a compatible PRL ERC-20; arbitrary/rebasing/upgradeable token behavior is not guaranteed. This new escrow contract has not undergone an external security audit or production deployment.

## Build and test

Foundry and Node/npm are required:

```bash
npm ci --prefix contracts
cd contracts
forge test -vv
```

The source is `contracts/src/LaunchStake.sol`; the committed ABI is `contracts/abi/LaunchStake.json`. Foundry compiles Solidity 0.8.30 for Cancun. Set the target appropriately and retest if the actual chain does not support Cancun.

The Sepolia test deployment uses the test-only tPRL token recorded in `config/deployments/sepolia.json`, not real PRL. Constructor parameters are `(IERC20 token, address admin)`. Prefer a multisig admin and separate verifier/settler identities. Do not put private keys in Git, CLI arguments or chat. Both test contracts have now been deployed; no provider stake or production token approval has been executed.

## Configure one default chain

In the repository root:

```bash
mkdir -p .secrets
cp config/chain.example.json .secrets/chain.json
```

The example now contains the recorded Sepolia test deployment:

```json
{
  "chain_id": 11155111,
  "staking_contract": "0xe5b29f59c9c90b7fbde165273a3464a35875c58c",
  "token_address": "0x3347f108f03287bdb39cba95f40f7f39e6dff1f8",
  "deployment_block": 12082860,
  "batch_size": 100
}
```

These are mined Sepolia test contract addresses; the token is tPRL. Sepolia ETH pays gas and is not the token being staked. Do not reuse a mainnet token address. Set `CHAIN_RPC_URL` in the operator environment or a protected service secret. The monitor verifies `eth_chainId` and the contract's `stakeToken()` against this configuration. The provider RPC used for AI inference is a different service from this blockchain RPC.

The RPC must support `eth_getLogs`, receipts, `eth_call` and the `finalized` block tag. It fails closed rather than treating `latest` as finalized. If the default chain uses different finality rules, implement and test that adapter explicitly.

## Create a draft and register its on-chain key

Initialize the central FRPS configuration if it does not yet exist (do not overwrite an existing one):

```bash
python3 scripts/frp_admin.py init --server frps.example.com --api-domain api.example.com
python3 scripts/stake_monitor.py project --owner 0xActualWalletAddress --name "My Qwen credits" --stake-raw 2000000000000000000000
```

The amount above is **only an example assuming 18 token decimals**. Use the actual PRL decimals and intended stake requirement. The tool prints a project UUID and `contract_project_key = SHA256(project UUID UTF-8)`. Register that bytes32 key, wallet and raw amount using the admin's contract transaction. The wallet then approves and stakes. This starter requires one project stake per direct contract transaction; batching multiple stakes or smart-account calls requires an explicit identity/event-index policy because Provider ID is the transaction hash.

`project` is currently an **operator bootstrap CLI**. A production authenticated project API should call the same durable project creation logic, authorize the wallet against the logged-in account, and then register the project on-chain. The React UI still uses simulated stake transactions; no real wallet submission or delivery download API has been wired into it.

## Start the event monitor

```bash
python3 scripts/stake_monitor.py once
python3 scripts/stake_monitor.py watch
python3 scripts/stake_monitor.py list
```

Or start the optional container profile after configuring the protected `CHAIN_RPC_URL` environment:

```bash
docker compose -f frp/server/compose.yaml --profile stake-monitor up -d
```

Run **one monitor/registry writer per deployment**. The SQLite file is the authoritative monitor state. Do not edit the exported registry concurrently; use lifecycle/backend transactions. Other backend writers must use the same transaction/storage boundary.

For each finalized event, the monitor verifies chain, staking contract, PRL token, receipt success, canonical block hash, unique stake event, project key, wallet, exact amount, block timestamp, deadline and current on-chain stake state. Unknown project events pause scanning so projects can be synchronized before resuming. RPC errors pause/retry without advancing the cursor. Expired/refunded stakes never receive a new deployment identity.

The checkpoint, event deduplication, project/provider/launch records and private configuration outbox are committed together in SQLite. Export happens after commit. On restart, the outbox re-exports the **same** Launch ID and credential, including after an interrupted file export.

## Delivery and private access

A confirmed stake creates:

```text
.secrets/frp-server/deliveries/<launch-id>/
  frpc.toml
  ca.crt
  token
  launch.json
```

`launch.json` contains project ID, Launch ID, Provider ID, uniform API URL and the original stake-based deadline. Other files contain private deployment credentials; directory/file permissions are restricted. The SQLite outbox also contains private credentials and must be protected and backed up like a secret store. It is not encrypted by this starter.

Deliver **only that launch's bundle** to its authorized owner through an authenticated private channel. Never serve the deliveries directory as static web content or put it in the public React assets. Owner-only download endpoints, credential rotation and vault encryption remain to be implemented. A public stake transaction hash is not authorization to retrieve its bundle.

After delivery, the provider copies the FRPC files to `.secrets/frp-provider/` and runs the [required FRPC hosting flow](HOSTING.md).

## Acceptance, expiry and revocation

`Monitor.accept_rpc(projectId, probe)` re-verifies the stake, requires a real backend probe, checks the finalized on-chain verification state and enforces completion before the deadline. Both the verifier transaction/finality and backend acceptance must fit within the original window; confirmation latency never extends it.

The monitor checks on-chain state and expires pending identities during polling. Refunded/released/invalid stakes disable provider and launch routes. Contract refunds are provider transactions; the monitor does not move funds. Established tunnels may remain connected, but disabled launch routes reject new public calls. Requests already running are not forcibly interrupted.

If an already-finalized checkpoint changes, the monitor disables routes and stops for operator reconciliation rather than silently issuing replacement IDs. The model remains dependent on the configured RPC's correctness; multiple-RPC verification and production monitoring/alerting are not supplied.

## Full local integration test

```bash
forge build --root contracts
RUN_ANVIL_TESTS=1 python3 -m unittest discover -s tests -p 'test_stake_monitor_anvil.py' -v
```

The test starts a disposable Anvil node, deploys mock PRL and the actual stake contract, registers a project, approves/stakes, finalizes blocks, and checks Launch ID issuance, tx-hash Provider ID, block-based deadline, export crash recovery, replay deduplication, wrong-chain/token rejection and refund-driven route disabling. Only Anvil test balances are used.
