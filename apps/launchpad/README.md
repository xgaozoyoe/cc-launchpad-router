# AI Computing Credit Launchpad UI

The React 19 + TypeScript + Vite application used by the Launchpad demo, with Auth.js Google sign-in and Node/Vercel API handlers.

## Screens included

- Login page and signed-in overview, with the downloadable PowerPoint white paper.
- Supported models and provider offers, filtered by model and sorted by price.
- Model/launch-specific credit balances and simulated inference receipts.
- My launches: account-owned projects, stage filters/search, resume staking, Launch IDs, transactions and block-based RPC deadlines. A checklist shows completed steps in green, the current step in yellow and later/blocked steps in red; text labels and symbols accompany the colors. Saved RPC passes advance the displayed next step to capacity/on-chain acceptance without changing the underlying sale status.
- Project-specific provider setup guide: assigned IDs/deadline, local API checks, FRPC bundle requirements and native connector commands, downloadable public checklist, and saved-state refresh. Owner-only Linux FRPC bundles and historical login/proxy observations are integrated; functional Test RPC results are saved, while live heartbeat and formal RPC/capacity acceptance remain unavailable. See [RPC checks](../../docs/RPC-CHECKS.md).
- PRL collateral display in project cards/details: locked, free/withdrawable and returned. Partial unlocking is not implemented.
- Compute-resource setup wizard, Docker download and endpoint configuration.
- Simulated PRL wallet and public compute pool.

## Run locally

Use Node.js 24 LTS and npm.

```bash
cd apps/launchpad
npm ci
cp .env.example .env.local
```

Configure Google OAuth using [GOOGLE-SIGN-IN-SETUP.md](GOOGLE-SIGN-IN-SETUP.md). Set `APP_ORIGIN=http://localhost:4173` for local development and use the matching Google callback URL. The app fails closed when sign-in is not configured.

```bash
npm run dev -- --port 4173
```

Open `http://localhost:4173`. The Vite development middleware also serves the authentication API.

## Validate

```bash
npm test
npm run build
```

The build checks TypeScript, compiles and smoke-tests the Node authentication endpoint, and builds the frontend.

## Deploy to Vercel

Set the project's **Root Directory** to `apps/launchpad`, use `npm ci`, build with `npm run build`, and serve `dist`. The app's `vercel.json` includes the authentication rewrites. Configure server-side Google credentials, session secret and `APP_ORIGIN` for the actual deployed domain; register that domain's Google callback URL. See the setup document for details.

The active integrated deployment is the native Ubuntu stack at https://riddle.xyz; see [remote operations](../../deploy/remote/README.md). Vercel support remains an alternative; its environment and deployment do not update automatically from the Ubuntu instance or a Git push.

## Architecture

- `src/App.tsx` — application navigation and account-specific demo state.
- `src/components/` — buyer, supplier, wallet and resource setup interfaces.
- `src/state.ts` — simulated ledger and transaction rules.
- `src/models.ts` — model catalog and offer comparison.
- `src/resources.ts` — browser-local resource configuration.
- `src/AuthGate.tsx`, `src/auth-client.ts` — sign-in interface and session requests.
- `server/`, `api/auth.ts` — server-side Google authentication.
- `public/whitepaper/` — 10-slide PPTX white paper.
- `public/supplier-demo/` — the downloadable supplier demo snapshot.

## Integration status

In the integrated workspace, Create project saves an account-owned MongoDB draft. Wallet binding, server registration, provider approve/stake transactions and finalized Sepolia event processing reserve the Launch ID. My launches displays these records and returns from project details to the dashboard. The old `#supplier` URL resolves to My launches.

The active flow stops at `awaiting-rpc`. RPC/model/capacity acceptance, credit sales, a buyer ledger, metering/burning and settlement are not connected. Resource records and buyer/PRL wallet/pool balances remain browser-local simulations. The separate Python FRP registry is not automatically synchronized with Rust/MongoDB. See [implementation status](../../docs/IMPLEMENTATION-STATUS.md).

The current escrow has no partial unlock ledger. Free/withdrawable is therefore zero for a recorded stake; a full release/refund transfers the entire amount directly back to the wallet. Draft/unconfirmed collateral is shown as unknown, not a confirmed deposit. The agreed proportional-release rule is a future contract/settlement change.

Never put Google client secrets, inference API keys or session secrets in React code, `VITE_` variables, the public directory or Git. `.env.example` contains configuration placeholders only; real environment files are ignored.
