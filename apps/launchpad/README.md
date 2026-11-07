# AI Computing Credit Launchpad UI

The React 19 + TypeScript + Vite application used by the Launchpad demo, with Auth.js Google sign-in and Node/Vercel API handlers.

## Screens included

- Login page and signed-in overview, with the downloadable PowerPoint white paper.
- Supported models and provider offers, filtered by model and sorted by price.
- Model/launch-specific credit balances and simulated inference receipts.
- Supplier studio and credit creation.
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

The current live Vercel deployment was uploaded separately. Adding these files to GitHub does not automatically link or redeploy that project.

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

Credit creation now uses the stake-gated project flow: draft → simulated stake confirmation → Launch ID and tx-hash Provider ID → RPC acceptance within 24h of the simulated stake block → sales. The separate resource guide still has generic setup content. The required FRPC/unified launch-ID routing is implemented at the repository root and documented in [HOSTING.md](../../docs/HOSTING.md). Wiring UI launch creation to that registry and updating the wizard to issue FRPC bundles remain pending.

Balances, transfers, capacity checks, credit issuance and inference in the UI are simulated. Resource records stay in this browser. No real PRL transactions, on-chain burns, provider probes or shared production ledger are implemented.

Never put Google client secrets, inference API keys or session secrets in React code, `VITE_` variables, the public directory or Git. `.env.example` contains configuration placeholders only; real environment files are ignored.
