# Project RPC checks

**My launches → Setup guide → Test RPC** runs functional API checks through the project's assigned FRPC/FRPS route. It does not accept a provider URL or bypass the project tunnel. The UI requires an active stake, an observed proxy registration and an inference API key.

## What is checked

1. Health responds through the deterministic project Host route.
2. Missing and incorrect inference keys are rejected (401/403).
3. The supplied key can discover the expected served model ID, `qwen3-8b-awq`.
4. A small non-streaming request returns a nonempty complete answer.
5. Positive prompt/output token counts sum to the reported total.
6. SSE returns content, a completion marker and consistent final usage.

The test uses a fixed small prompt and at most 48 output tokens per generation request. Generation and streaming each invoke the model once. Results contain pass/fail details and elapsed time, not the API key or generated output. Model discovery only checks the advertised identifier; it does not attest the actual weights, quantization, hardware ownership or available capacity. Usage arithmetic is not proof of honest metering.

The provider submits the inference key over authenticated HTTPS for the current test only. The frontend clears the field when testing starts. Rust uses the key in memory and does not put it in MongoDB, files, logs, responses or project records. There is no persistent credential vault or buyer credential setup in this feature.

## Access, routing and limits

The same-origin session gateway signs the verified owner identity. Rust enforces account/legacy-alias ownership and validates the active finalized stake against current contract state and chain time before and after testing. Inactive or expired projects cannot start a check. Other-owner requests return 404. Unknown input fields, including alternative URLs, are rejected.

Probe traffic goes to loopback FRPS HTTP ingress with the project-derived Host alias. Redirects and environment HTTP proxies are disabled. No provider-controlled hostname or arbitrary port is accepted in the request. The trusted operator may configure a loopback `RPC_CHECK_PORT` (default 8080); tests use a disposable local mock port.

There is one MongoDB-backed test lease per project, bounded response sizes, HTTP deadlines, a 45-second overall probe deadline and a short retry cooldown. A crash leaves a lease that expires. Missing/failed checks appear as failed or not reached. Technical results are saved in `rpc_checks`, independently of project sale status, and remain readable by the owner after expiry as historical evidence.

## API

- `POST /api/v1/projects/<id>/deployment/rpc-check`: body `{ "inference_api_key": "..." }`; returns the saved pass/fail report. Failed probes are reports, while invalid/ineligible requests return HTTP errors.
- `GET /api/v1/projects/<id>/deployment/rpc-check`: saved report and in-progress flag, no key.
- Browser routes use `/api/central/…`; POST requires the application Origin.

**A pass does not call `verifyProject`, accept capacity, change `awaiting-rpc` to live, extend the deadline or enable sales.** Capacity/service commitment criteria, on-chain acceptance and paid credit offering/consent remain separate steps. The paid test gateway is documented in [credit spending](CREDIT-SPENDING.md). An old successful report is not continuous health monitoring.

## Launch-card progress

Owner-filtered project list/detail responses include only sanitized progress fields (`bundle_issued`, `last_proxy_at`, `rpc_status`, `rpc_checked_at`) joined from deployment and saved-check records. Credentials and report bodies are not copied into project records. The dashboard shows completed steps green, the current step yellow and later steps red, with matching text/symbols. Inactive projects have blocked incomplete steps and no active yellow step. A saved API pass changes the display to awaiting capacity acceptance and shows its test time; it does not change `awaiting-rpc`, verify capacity or open sales. Connector registration and test ticks are historical evidence, not continuous health.

## Optional platform custody and admin rechecks

The original one-time key behavior remains the default. Providers may now explicitly authorize private backend storage for future checks and revoke it in setup. An admin can then sign **Retest RPC** directly from review without using the publisher’s account or seeing its key. Both entry points use the same probes, leases, cooldown, saved results and chain eligibility. See [credential storage, consent and limitations](ADMIN-REVIEW.md#admin-rpc-rechecks). The recheck does not approve capacity or submit `verifyProject`.
