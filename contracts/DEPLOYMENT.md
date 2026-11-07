# Deploy test staking contracts with Forge

Deploys **test-only tPRL** and `LaunchStake` on Ethereum Sepolia (11155111). tPRL is not real PRL. The script refuses mainnet. The initial supply is 1,000,000 tPRL to the administrator; only that administrator can mint more test tokens.

No private key is read by the Solidity script. Use an existing encrypted Foundry keystore or a supported hardware signer. Do not commit/paste a private key or put one in command-line arguments.

From the repository root, install contract dependencies if missing:

```sh
npm --prefix contracts ci
forge test --root contracts -vv
```

Set `CHAIN_RPC_URL` and the **public** `DEPLOYER_ADDRESS` in your shell. Optional `STAKE_ADMIN`, `STAKE_VERIFIER` and `STAKE_SETTLER` select role holders (all default to the deployer). The script grants the configured roles and renounces its temporary deployer administrator role when a different administrator is specified. Preserve these same environment values for verification/export.

```sh
export CHAIN_RPC_URL=https://ethereum-sepolia-rpc.publicnode.com
export DEPLOYER_ADDRESS=0xYOUR_PUBLIC_DEPLOYER_ADDRESS
# Preflight: no transactions are broadcast.
forge script --root contracts contracts/script/DeploySepolia.s.sol:DeploySepolia \
  --rpc-url "$CHAIN_RPC_URL" --sender "$DEPLOYER_ADDRESS"
# Actual Sepolia transactions: encrypted account, password entered locally.
forge script --root contracts contracts/script/DeploySepolia.s.sol:DeploySepolia \
  --rpc-url "$CHAIN_RPC_URL" --sender "$DEPLOYER_ADDRESS" \
  --account YOUR_KEYSTORE_NAME --broadcast --slow
```

Fund this signer with Sepolia ETH first. Review the simulation's gas estimate before broadcasting. No mainnet funds or real PRL are needed. If broadcasting is interrupted, inspect the recorded receipts before using Forge's `--resume`; do not blindly run a new deployment and lose track of earlier contract addresses.

After successful mining, verify all receipts, runtime code, token binding, token ownership and configured roles, then export a public manifest:

```sh
python3 scripts/export_stake_deployment.py \
  --broadcast contracts/broadcast/DeploySepolia.s.sol/11155111/run-latest.json \
  --output config/deployments/sepolia.json
```

The exporter refuses another chain, missing/reverted transactions, noncanonical receipt blocks, mismatched token/roles, and an existing output file. The manifest contains no RPC URL or signer secrets. It records mined deployment receipts; the event monitor must still wait for finalized blocks before accepting stakes.

Anvil rehearsal uses chain 31337, `ALLOW_LOCAL_DEPLOY=true`, an Anvil unlocked account, and Forge's `--unlocked --broadcast`. Export with `--allow-local` into a separate local-only file. Do not confuse a local deployment with a Sepolia deployment.

Deployment alone does not connect the Rust API to stake events or enable sales. Wallet ownership, project registration, finalized indexing, Launch ID issuance and RPC acceptance still need to be integrated before a real launch can complete.

## Recorded Sepolia deployment

The verified public record is [`config/deployments/sepolia.json`](../config/deployments/sepolia.json). `config/chain.example.json` now points to this **test-only tPRL** deployment. The existing Rust service still does not index stake events or enable credit sales.

The Ubuntu deployment command is `python3 ~/services/cc-launchpad/deploy/remote/deploy-stake.py` for simulation, with `--broadcast` for real Sepolia transactions. It reads the dedicated encrypted keystore and password file from `private/sepolia-wallet/` and refuses another deployment after a manifest is present. A prior broadcast requires explicit inspection/resume before any retry.

During the first Sepolia attempt, the creation transaction exhausted the default gas limit and reverted. Its receipt and broadcast artifacts were archived under the remote `private/failed-deploy-*` directory. The node estimated approximately 4.18 million gas for creation, above the local EVM estimate; the successful retry used a larger gas-estimate margin. A gas limit is an upper bound; fees are charged for gas actually consumed.
