# Stake-gated project launch

The publishing sequence is:

1. Create a project draft: choose model, name and intended terms. Only an internal project ID exists.
2. Stake on the platform's **default chain**. The trusted server adapter verifies the successful canonical staking transaction/event, contract, owner, project binding, amount and required confirmations.
3. After acceptance, issue the Launch ID and a private deployment credential. **Provider ID is the full normalized stake transaction hash.** One stake transaction may back only one project in this starter.
4. Complete provider RPC/model/usage/capacity acceptance before `stake_block_timestamp + 86400`.
5. Mark the launch live and open credit sales. The same Launch ID remains in its uniform API URL.

The deadline starts at the on-chain stake block timestamp, not wallet submission, platform confirmation, credential issuance or the first FRPC connection. Confirmation and provisioning consume part of the 24-hour window. At exactly the deadline acceptance is rejected. Retrying confirmation does not reset the deadline or issue another secret.

The stake transaction hash is public identity, never a secret. The generated private credential belongs to this stake-backed project/launch. Suppliers must hold it to register the assigned tunnel. Because a 66-character `0x` transaction hash exceeds a DNS label, FRP uses a deterministic internal alias `p-<40 hex characters of SHA256(provider_id)>`. The full hash remains the registry Provider ID and FRPC user; the alias is checked server-side. Buyer URLs continue to use Launch ID, not the alias.

## Current code and boundaries

- `frp/server/lifecycle.py`: backend state machine, injected default-chain verifier and RPC acceptance probe, server-clock deadline enforcement, credential hash and identity issuance, sale eligibility.
- `frp/server/registry.py`: validates the project-specific credential and ID binding; pending-deployment credentials stop admitting new sessions after the deadline.
- `frp/server/router.py`: only `live` launches reach public inference; acceptance probes must use private operator routing before activation.
- `apps/launchpad/src/projects.ts`, `state.ts`, `components/ProjectLaunch.tsx`: browser-local demonstration of this exact ordering. Mock stake hashes and timestamps are explicitly labeled. Only accepted projects enter the demo sales list.

**The development default chain is Sepolia (11155111), but the deployed staking contract and test PRL addresses are not configured yet.** The EVM implementation and monitor are available in [STAKING-CONTRACT.md](STAKING-CONTRACT.md). The server lifecycle refuses stake confirmation without a trusted chain verifier. No browser-supplied hash, timestamp, `verified` flag or simulated confirmation is valid server evidence. The event monitor now integrates the EVM verifier and lifecycle in durable SQLite transactions and exports private deployment bundles after confirmed stakes. Production still requires actual chain/token/contract configuration and authenticated owner-only delivery APIs. It does not expose a production project HTTP API yet.

The backend verifier interface receives `(tx_hash, project, default_chain)` and must return verified `chain_id`, `contract`, `tx_hash`, `project_id`, `owner`, `amount`, `block_timestamp` and `block_hash`. A raw user JSON object is not an adapter. Before RPC acceptance the stake is verified again to detect reorg/inconsistent receipts. The RPC probe must perform actual auth/model/usage/capacity checks, not accept client-provided booleans. The lifecycle requires the verified amount to equal the project stake requirement; the adapter must normalize chain decimals consistently.

The current hash-format guard is for 32-byte `0x` transaction IDs. If the actual default chain uses a different transaction format, its adapter/normalizer must be implemented explicitly; this code does not choose Ethereum or another chain on the user's behalf. Configure one chain per registry; use chain + tx identity if multi-chain support is added.

Legacy CLI `add`/`launch` issuance is disabled to avoid skipping stake acceptance. `init` remains available for central FRPS infrastructure and `list` for inspection. Existing legacy records require an audited migration before being marked live; the router no longer treats a missing status as live.

Expired projects remain unsellable, with no automatic slashing or refund. Production contract refund/cancel rules remain to be implemented. A live connection is not proof of model ownership or capacity. Credentials may be copied; device binding is not implemented.

## UI demo

Open Supplier studio → Create / resume project. Create a draft, simulate stake confirmation, view the issued Launch ID, Provider ID, stake block time and deadline, then simulate RPC acceptance. This performs no chain transaction or provider probe and generates no usable server deployment secret. The website's state remains local and is not synchronized with the backend library.
