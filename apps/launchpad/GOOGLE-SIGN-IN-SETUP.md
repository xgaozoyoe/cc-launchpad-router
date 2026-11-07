# Enable Google sign-in

The React/TypeScript implementation uses Auth.js with Google OAuth, PKCE, state,
nonce verification, an encrypted HTTP-only session cookie, and configurable Google account access.
The Google client secret stays on the server. No database is needed for this
demo's login sessions. Compute balances remain simulated, browser-local data,
separated by Google account ID. This is not a backend for real token balances.

Active integrated app URL: https://riddle.xyz

The Vercel deployment is an alternative/earlier demo, configured separately. The production Ubuntu gateway reads private server configuration; see [remote operations](../../deploy/remote/README.md).

Vercel settings: https://vercel.com/sinka/ai-computing-credit-launchpad/settings/environment-variables

## 1. Create a Google Cloud OAuth application

Open [Google Auth Platform](https://console.cloud.google.com/auth/overview) and
select or create a Google Cloud project.

- Under **Branding**, set the app name to **AI Computing Credit Launchpad** and
  supply your support/contact email.
- Under **Audience**, choose **External** if people outside your Google Workspace
  will use it. Configure publishing status, test users and organization policies
  for the intended audience in Google Auth Platform.
- Under **Clients**, create an OAuth client of type **Web application**.
- Add the deployed app's HTTPS origin under **Authorized JavaScript origins**.
- Add this exact **Authorized redirect URI**, replacing the origin with the
  deployment's stable domain:

```text
https://riddle.xyz/api/auth/callback/google
```

For local development, add both of these as well:

```text
Authorized JavaScript origin: http://localhost:4173
Authorized redirect URI:      http://localhost:4173/api/auth/callback/google
```

Use a stable production domain. Each random preview hostname would need a
matching redirect URI. This implementation only requests basic identity scopes:
`openid`, `email`, and `profile`.

## 2. Configure the application server

In the hosting project's environment-variable settings, configure:

| Variable | Value |
| --- | --- |
| `GOOGLE_CLIENT_ID` | Web application client ID from Google |
| `GOOGLE_CLIENT_SECRET` | Client secret from Google |
| `AUTH_SECRET` | A fresh, strong random secret with at least 32 characters |
| `APP_ORIGIN` | The exact HTTPS origin, for example `https://riddle.xyz` |
| `AUTH_ACCESS_MODE` | `public` to allow all verified Google accounts |
| `AUTH_ALLOWED_EMAILS` | Optional comma-separated list when using `private` mode |

Generate the session secret locally with `openssl rand -base64 32` and enter it
directly into the host's secret settings. Do not put either secret in a `VITE_`
variable, source control, browser code, or this chat.

For local development, copy `.env.example` to `.env.local`, fill in the values,
set `APP_ORIGIN=http://localhost:4173`, and run `npm run dev`. Environment files
ending in `.local` are ignored by Git. Restart the server after changing them.

Public mode allows any verified Google account at the application layer. Google
account and OAuth audience policies still apply. Private mode
requires an explicit email allowlist.

## 3. Deploy to independent hosting

The repository includes a Vercel Node function and route rewrites. Project:
`ai-computing-credit-launchpad`, team: `eco-projects` (`sinka`).

The Ubuntu deployment uses the same Google OAuth flow through its Node gateway. The stable subject is derived from the Google provider account ID; legacy signed sessions can be linked on verified reauthentication. Project ownership is stored in MongoDB, independently of the browser-local simulated credit ledger.

On Vercel, use a custom production domain or review Deployment Protection so the
production app does not add a separate Vercel-account login in front of Google.
Keep preview protection enabled where appropriate. Configure Google and all
environment values before granting access to the login-enabled app.

## 4. Verify the complete flow

Redeploy after setting production environment variables. Then:

1. Open the independent deployment and select **Continue with Google**.
2. Sign in with a Google account. In private mode, the account must also be in
   the application allowlist.
3. Confirm your account name/email appears in the launchpad sidebar.
4. Refresh the page to check session persistence, then use **Sign out**.
5. In public mode, confirm a second verified Google account can sign in. In private
   mode, confirm a non-allowlisted account cannot access the workspace.

Until credentials and the origin are configured, the app fails closed and clearly
shows that Google sign-in is awaiting configuration. No simulated login bypass
or fabricated authenticated session is available.

References: [Auth.js Google provider](https://authjs.dev/getting-started/providers/google),
[Google OAuth web-server configuration](https://developers.google.com/identity/protocols/oauth2/web-server).
