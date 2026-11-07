# Wallet binding and finalized staking in Rust

The active production path is React → Auth.js session gateway → signed Rust API → MongoDB, with a Rust background worker reading Sepolia. The older Python/SQLite monitor remains a reference implementation and is not the production writer.

## Flow

1. Create a model-specific project draft (internal UUID, no Launch ID).
2. Connect an Ethereum EOA wallet on Sepolia and sign a project-bound EIP-191 challenge. The message binds the account, origin, UUID, chain, token, staking contract, stake amount, nonce and five-minute expiry. It authorizes wallet binding only, not a token transfer. The challenge is atomically consumed once.
3. The server registers `SHA256(project UUID UTF-8)` with the existing stake contract. Only the configured operator signs this administrative transaction. The provider and amount are immutable after registration. A retry reads chain state before sending, preventing duplicate registration after a crash.
4. The provider approves exactly the committed tPRL amount and sends `stake(bytes32)`. Browser transactions are signed by the provider's own wallet. The platform never asks for its key.
5. The API records the transaction hash after checking sender, recipient and calldata. The worker also discovers finalized `ProjectStaked` logs independently, so closing the browser after sending does not lose the stake.
6. Only the `finalized` block tag is accepted. The worker rechecks receipt success, direct provider transaction, canonical block hash, single stake event, amount, owner, event timestamp, and current/finalized contract state. There is no fallback to `latest` for finality.
7. A MongoDB atomic project update issues a unique Launch ID, records Provider ID as the normalized full stake transaction hash, and sets `rpc_deadline = stake block timestamp + 86400`. Sparse unique indexes and a compare-and-set update prevent duplicate IDs or provider-hash reuse. Replays cannot reset the deadline.
8. The project enters `awaiting-rpc`. RPC/model/capacity acceptance and actual sales are not enabled by this implementation. Expiry uses the latest chain block timestamp, rather than trusting the application server clock. Expired, refunded or released stakes stay inactive. An already refunded stake encountered during catch-up does not block the event cursor or receive a new Launch ID.

The worker holds a MongoDB lease, renewed between phases/events. It checkpoints finalized block number/hash. A changed finalized checkpoint halts processing and disables active reservations pending operator investigation; it does not silently reset the cursor. Restarting a worker can require up to five minutes for the previous lease to expire.

## Runtime configuration

Add these server-only values to the backend environment (the Ubuntu supervisor reads `private/config.json`):

```text
CHAIN_CONFIG_FILE=/home/xgao/services/cc-launchpad/deploy/sepolia.json
CHAIN_RPC_URL=https://ethereum-sepolia-rpc.publicnode.com
APP_ORIGIN=https://riddle.xyz
STAKE_CAST=/home/xgao/services/cc-launchpad/tools/foundry/cast
STAKE_KEYSTORE=/home/xgao/services/cc-launchpad/private/sepolia-wallet/keystores/sepolia-deployer
STAKE_PASSWORD_FILE=/home/xgao/services/cc-launchpad/private/sepolia-wallet/password
```

`CHAIN_CONFIG_FILE` includes chain ID, deployed token/escrow addresses and deployment block. Other public manifest fields are ignored. Startup validates chain/token binding and confirms the registration signer has `DEFAULT_ADMIN_ROLE`. The operator key remains in an encrypted keystore; its local unlock file and all configuration stay inside the private directory. No key or password is logged, returned to clients or passed as a CLI argument.

Without chain configuration, draft APIs still work and stake endpoints return unavailable. With incomplete/wrong configuration the service fails closed. Only Sepolia is accepted by default; disposable Anvil requires `CHAIN_ALLOW_LOCAL=1`, chain 31337 and localhost HTTP.

## Authenticated API

All requests retain the existing server HMAC envelope, replay protection and project owner checks. The browser uses `/api/central/…`; the gateway derives identity from the verified Google session and does not trust browser identity headers.

| Method | Backend path | Purpose |
| --- | --- | --- |
| GET | `/api/v1/status` | Chain readiness, finalized checkpoint and monitor errors |
| GET | `/api/v1/projects/<id>/stake` | Owned project, balance and fixed approve/stake calldata |
| POST | `/api/v1/projects/<id>/stake/challenge` | `{wallet_address, stake_amount}`; whole test tPRL, 1000–100000 |
| POST | `/api/v1/projects/<id>/stake/prepare` | `{challenge_id, signature}`; verify wallet and queue registration |
| POST | `/api/v1/projects/<id>/stake/submit` | `{tx_hash}`; record a direct provider stake transaction |

Clients cannot assign a provider ID, Launch ID, finalized flag, RPC-verified state or sale status. The worker is the only issuer. A reverted submitted stake returns to `awaiting-stake`, retaining the failed transaction hash in history so it can be retried safely.

## Tests and limits

```sh
cargo +stable test --manifest-path central/Cargo.toml --locked
npm --prefix apps/launchpad test
npm --prefix apps/launchpad run build
```

`tests/stake_rust_e2e.py` runs the actual Rust service against disposable authenticated MongoDB and Anvil. It checks incorrect signatures, consumed challenges, owner isolation, registration, approvals/stakes, no premature ID, finalized issuance, immutable deadline, cross-project hash rejection, refunds before/after finality and finalized-checkpoint reorg halting. It requires `eth-account`, Foundry, MongoDB and built Rust/contract artifacts; no Sepolia funds are used by this local test.

The operator's `deploy/remote/test-sepolia-stake.py --account <verified application ID> --wait 1200` uses the dedicated funded Sepolia test wallet. It saves resumable state privately and a public test report only after finalization. Re-running after a recorded stake only waits/checks; an interrupted send without a recorded receipt requires manual inspection before another transaction.

EOA signatures are supported; EIP-1271 smart-contract wallets are not yet supported. Test tPRL must be issued by its administrator, and provider wallets need their own Sepolia ETH for gas. This release does not automate test-token faucets, FRPC credentials, RPC acceptance, credit settlement, slashing or production PRL transfers. All purchases/balances outside server projects remain concept simulations.

## Login identity persistence

Auth.js uses a stable `google:<provider account ID>` subject for new Google sessions. The earlier no-database login created transient UUID subjects, which could hide projects on a later login. A user with an existing valid legacy cookie can choose **Refresh sign-in**; only when the old signed session email matches the newly verified Google email does the server create an immutable legacy-to-canonical account link.

That link endpoint requires the server HMAC and is deliberately absent from the browser proxy allowlist. Project queries include authenticated aliases; legacy requests are resolved to their canonical identity. No project documents, stake amounts, chain keys or deadlines are rewritten. A legacy account cannot be reassigned to another Google identity. Expired or missing legacy sessions need separate operator-assisted recovery; the system does not guess ownership from client-supplied email or UUIDs.

## Recorded Sepolia test

[`config/test-results/sepolia-stake.json`](../config/test-results/sepolia-stake.json) records the actual 2,000 tPRL test stake and finalized Launch ID. The provider is the dedicated funded test deployment wallet. The project appears under the signed-in owner's account; its public report excludes the account identity and all signing secrets. The same Launch ID and deadline were read back after backend restart and fresh Google authentication. This test stops at `awaiting-rpc`; it does not claim GPU capacity, FRPC connectivity or credit sales have passed acceptance.
