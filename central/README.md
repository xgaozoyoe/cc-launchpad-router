# Rust central service

The central backend uses Rust, Axum, Tokio and MongoDB. Draft APIs, project-bound wallet signatures, administrative chain registration and finalized Sepolia stake indexing are now connected. A finalized valid stake reserves a Launch ID and a deadline fixed at the stake block time plus 24 hours.

See [the active staking guide](../docs/RUST-STAKING.md) for configuration, API routes, idempotency, finality/reorg handling and tests. The backend always reports sales disabled: RPC/model/capacity acceptance and credit settlement are not connected yet.

[Local and single-host startup](../deploy/stack/README.md) covers the integrated React/auth/backend/database stack. For the deployed Ubuntu instance, see [remote operations](../deploy/remote/README.md). An authenticated MongoDB replica set or Atlas is required; standalone MongoDB fails startup.

Requests require a server-only HMAC envelope. The React app calls the same-origin session gateway, which verifies Google login and signs the verified account identity. Never put the HMAC key, signing keystore or its unlock file in browser code.

Collections include `projects`, `request_nonces` and `chain_state`. Unique sparse indexes protect project on-chain keys, provider transaction hashes and Launch IDs. The MongoDB project document is the atomic issuance record, so a failed or replayed cursor batch cannot create a second ID.

```sh
cargo +stable test --manifest-path central/Cargo.toml --locked
```

The optional MongoDB-only test uses a disposable replica set on port 27187 (`cargo test -- --ignored`). The full local stake test additionally needs Anvil, built contract bytecode and an `eth-account` environment; see `tests/stake_rust_e2e.py`.
