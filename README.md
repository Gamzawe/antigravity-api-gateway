# Antigravity Private API Console

## Owner-console workflow

This is a private website for one owner, not a public developer platform.
Open the console, sign in, click Create API key, give it a name and copy it once.
Copy the API base URL into an OpenAI-compatible app. Choose a model ID from
Available models. Revoke keys individually in the website.

The gateway is unofficial. Antigravity OAuth access is not the official Gemini API.
Your Pro subscription is not unlimited API credit and does not unlock every model.
Account entitlements, quotas, service terms and possible account restrictions still apply.
Google AI Studio/API billing and consumer subscriptions are different products.
Do not publish or resell access without checking the applicable provider terms.

### Local use

Requires Node 24 or newer. Run npm ci, then npm start. Open the console URL
printed on startup (the existing local .env may use port 8089 rather than 8080).
On first use, choose your owner username and a password of 12+ characters.
No setup token is required. Exactly one owner account can be created; after that,
account creation is disabled, including after a server restart.
Create this account before making the console public: the first visitor who
completes setup becomes the owner.
Subsequent visits require only your owner username and password.
Do not send passwords or API keys in chat.

### VPS deployment: Docker Compose with automatic HTTPS

Deploy only to a VPS you control. Point your domain DNS at its public IP and allow
ports 80 and 443 (TCP, optionally 443 UDP). Docker Engine and Compose must be installed.
Transfer the source project, excluding node_modules, .git, .unlazy and local .env.
Transfer data/accounts.json separately over SSH only with your explicit permission:
it contains Google refresh tokens. Do not upload it to a public repository or image.
Before public deployment, create your owner locally and transfer
data/console/access.json privately over SSH as well. It preserves your owner login
and generated API keys. Do not expose an empty owner state to the internet.
The one-shot data-init service prepares only this project's data directory for
container UID 1000 (directories 700, files 600), refusing symlinks and special files.
Do not use a shared/system directory or erase an existing private data directory.

In the VPS .env set:
    PUBLIC_URL=https://api.yourdomain.com
    DOMAIN=api.yourdomain.com
    PORT=8080

Run bash deploy/install.sh from the project directory. It checks the configuration
and starts the gateway and Caddy. Caddy obtains TLS automatically when DNS and the
firewall are correct. Node listens internally on 8080; the host port is loopback only.
Do not start a second proxy on occupied ports. No VPS deployment has been verified
until the remote console, HTTPS and a generated-key inference request all pass.

Only the configured owner can sign in. There is no second-account registration
or public password-reset endpoint. If starting from fresh state on a VPS, keep it
private until you have created the owner; otherwise the first visitor can claim it.
Daily use is only: open website -> sign in -> create/copy key -> use app.

### Security and maintenance

API keys created in the console are stored only as hashes. Full keys are shown once. Revocation blocks new requests, not in-flight calls.
The data/console/access.json file holds the password hash and key hashes/metadata.
Back up the entire data directory privately and encrypted; it includes refresh tokens.
Sessions are HttpOnly and SameSite Strict, use Secure cookies on HTTPS and expire
in 12 hours. Restarting the server logs out browsers but preserves API keys.
State storage supports one Node process per data volume, not multiple replicas.
The 200-key limit counts all retained records, including revoked keys.
Lost owner password recovery requires trusted SSH access and a backed-up reset of
owner state; there is no public reset endpoint. Existing environment API_KEYS still
work for inference only, but cannot be revoked in the website; remove them from .env.

Public /health is liveness only and never queries or exposes Google accounts.
/v1/key-check validates an application key without using model quota.
Account limits, detailed health and management routes require the owner session.
Application API keys cannot create other keys or manage accounts.
Local file URLs are rejected; use inline base64 for supported attachments.
Provider failures, quota limits and features other than the tested paths remain possible.

### Verification commands

    npm run test:console
    node scripts/verify-console-live.mjs
    node scripts/verify-console-browser.mjs
    node scripts/verify-console-container.mjs

Live tests use a separate temporary state directory and a private read-only copy
of the local account export, never the original OpenCode account store. Browser
tests use a separate temporary owner and do not create credentials for the user.
Container checks run locally; they do not verify a public TLS certificate or VPS.
The simple global login throttle can temporarily block owner login under attack.

### Attribution

The inference adapter derives from johnneerdael/antigravity-gateway (MIT).
Retain the included LICENSE and upstream notices.

---

## Endpoints and verification limits

Applications authenticate with Authorization: Bearer YOUR_API_KEY or x-api-key.
The base URL is your public origin plus /v1. Routes include /v1/models,
/v1/chat/completions, /v1/messages and /v1/responses. The console live smoke test
covers OpenAI chat text, SSE and tool continuation, not every endpoint, model,
reasoning mode or attachment format. Aliases are not evidence of entitlement.

Run node scripts/verify-console-container.mjs to exercise the built Docker image,
non-root permissions, secure cookie flags and persisted keys across restart.
This does not verify a real VPS deployment or public TLS certificate.

Account provisioning is a separate trusted server operation, not public website
registration. Existing account CLI commands remain available on a trusted host.
An app key cannot reload accounts. Keep a private backup and trusted SSH recovery
path. Remote local-file reads and server-side PDF rendering are not supported.
