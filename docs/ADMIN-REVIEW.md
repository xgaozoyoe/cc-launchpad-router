# Wallet-authorized launch review

Open `/#admin` after Google sign-in. Connect an Ethereum browser wallet. The app requests the chain configured in the backend challenge before asking for the login signature; confirm the switch in your wallet. If Sepolia is not installed, the app offers to add it and checks the resulting chain. Cancellation or a wallet remaining on the wrong chain stops login. Network changes initiated outside this connection flow still clear the admin session. Then sign the login challenge. Only the configured `ADMIN_WALLET_ADDRESS` can access project reviews. Google identity is used for account binding and audit; email alone grants no admin authority. EOA wallets are supported; EIP-1271 contract wallets are not supported.

## Review and verify

1. The provider must have a finalized stake, a reserved Launch ID and a passing Test RPC report for the same Provider ID, checked within the last hour.
2. An admin enters a reason and explicitly approves a **manual demo capacity exception, not a measured benchmark**. The wallet signs the exact project, reason and operation. The backend recovers the signer and records wallet, account, time and reason. This does not certify throughput or allow sales.
3. The admin selects **Sign verifyProject transaction**. A second signed authorization is checked by the backend. It checks the active stake, original deadline, capacity decision, fresh RPC result and the wallet's on-chain `VERIFIER_ROLE`, then returns fixed transaction calldata.
4. The browser wallet submits `verifyProject(bytes32)` on the configured chain. The project key is SHA-256 of the internal project UUID, not the Launch ID or stake hash. No signing key is sent to the server.
5. The browser signs a tracking authorization for the transaction hash. The backend checks the transaction's sender, chain, contract, calldata and zero ETH value. The monitor independently reads on-chain state and marks the project `accepted` only after finalized state is Verified. A mined receipt alone is insufficient. Reverted attempts return to awaiting RPC (or expired).
6. `accepted` means **on-chain acceptance finalized; sales closed**. Buyer payment, credit issuance/accounting and settlement are separate unfinished integrations. Acceptance preserves FRPS eligibility after the original RPC deadline; refunded/released stakes and a halted chain monitor cannot authenticate new connectors.

The contract transaction must execute before the original stake-block timestamp + 24 hours. Finality may occur later; neither reconnecting FRPC nor an off-chain approval extends the deadline. The contract enforces the transaction deadline.

## Configuration and authority

Set `ADMIN_WALLET_ADDRESS` in the private Rust service environment. It has no default: a missing/invalid value fails closed. The configured EOA must also hold `VERIFIER_ROLE` on LaunchStake to submit verification. Browser chain switching does not grant contract roles. To use a different browser wallet, the contract administrator must grant that role and the operator must explicitly update this configuration. Do not import or expose the server's deployment key just to use this page.

The active app admin is `0x22b47B6689b34d88b9d259029561692cfD589442`; its Sepolia `VERIFIER_ROLE` grant is recorded in [the public result](../config/test-results/admin-wallet.json). The contract's DEFAULT_ADMIN_ROLE remains with the original test deployer. No project was verified by this setup.

The deployment's original admin/verifier is documented in `config/deployments/sepolia.json`. It is a dedicated test deployment wallet, so configure a browser-controlled authorized address before normal operator use if that wallet is not in your browser.

## Request security

All `/api/v1/admin/*` routes use POST and the existing authenticated HMAC gateway. Browser requests require Google sign-in and a matching Origin. Every mutation additionally requires a one-use signed challenge bound to application origin, Google account, admin wallet, chain, contract, operation, exact serialized payload, random nonce and a five-minute expiry. The backend stores the payload and executes that stored value; changing unsigned request fields cannot change an approval. Challenge consumption is atomic. Wrong-wallet signatures, payload tampering, cross-account use and replay are rejected.

Read-only review sessions expire after 15 minutes, are bound to account and admin wallet, are stored as hashes in MongoDB, and are kept only in browser memory. Wallet/account/network changes clear the displayed session. A valid review session cannot approve anything without a new wallet signature. Admin changes take effect immediately because every request checks the current configured address.

Collections: `admin_challenges`, `admin_sessions` (hashed tokens), `admin_audit` (append-only decisions). Project fields include `capacity_approval`, `verification_tx_hash`, and statuses `verification-pending` / `accepted`. These collections contain no private keys. Expired challenge/session records are rejected by timestamp even if retained in the database.

Routes: `challenge`, `authenticate`, `query`, `capacity`, `prepare`, `submit`, `retest`. Mutation bodies contain `challenge_id` and `signature`; action payloads are provided when requesting the challenge. Only `query` accepts the read session token.

## Verification

`tests/stake_rust_e2e.py` runs a disposable Anvil/MongoDB/backend stack. It covers non-admin signatures, replay, altered approval text, account-bound sessions, missing prerequisites/roles, browser-style transaction preparation, chain finality, and accepted-tunnel eligibility after the deadline. It spends no Sepolia funds. Frontend tests also cover gateway session/Origin guards and accepted-but-not-selling dashboard state.

## Admin RPC rechecks

Admins use **Retest RPC · sign admin authorization** on the review card. This is the signed `retest` operation with `{project_id}`; it does not impersonate the publisher. The same one-use challenge binds the admin wallet, account, origin, project, chain and operation. The backend loads the project-bound credential and invokes the same bounded probes, stake eligibility checks, project lease and cooldown as owner-initiated checks. It saves the same sanitized report and a secret-free `rpc-retest` admin audit event. No approval or blockchain transaction is performed by a recheck.

The publisher authorizes custody once in provider setup: check **Allow platform rechecks**, enter the inference key and run Test RPC. After a successful test, the UI saves it through owner-only `POST /projects/<id>/deployment/rpc-credential`. Unchecked tests remain one-time use. Owners can rotate by saving a new key, inspect an authorization boolean via GET, or revoke with POST `{revoke:true}` even after project expiry. Revocation prevents subsequent rechecks; a probe already in flight can finish within its existing 45-second bound.

Set `RPC_CREDENTIAL_DIR` to an absolute operator-private directory (0700), under the project’s private root. Keys are stored as atomic 0600 files, bound to the project UUID and Provider ID; a credential for one provider cannot be reused after identity changes. Symlink files, oversized/invalid files and group/world-accessible files/directories are rejected. Missing configuration fails closed. This is filesystem protection, **not application-level encryption at rest**; host operators and private backups must be trusted. Neither normal project responses, admin listings, probe reports nor MongoDB audit records return the key. `rpc_credential_events` records owner authorize/revoke actions without secret values. A provider should use a dedicated restricted inference credential where its server supports one; the platform must be trusted with the permissions that credential carries.

Admin listings return only `rpc_retest_available`. A missing credential requires the publisher’s one-time authorization, not switching the admin into the publisher’s account. The already hosted demo provider can be enrolled by its operator without exposing the key in the browser.

## Separate admin categories

`Deployment migration` contains deployment inventory and migration controls. `Waiting for approval` contains only launches with finalized stake, a provider identity, an unexpired RPC acceptance deadline and a passing RPC result from the previous hour (future timestamps are rejected). Projects without these prerequisites are shown in a separate collapsed waiting section with their blocker and no approval reason/checkbox/button. Pending on-chain verification and accepted launches have separate history sections, also without a new capacity approval form. The queue refreshes every ten seconds while signed in and idle. Backend capacity approval and transaction preparation still enforce `ready()` independently of frontend filtering.
