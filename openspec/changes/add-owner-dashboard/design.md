## Decisions
Use the existing Express server and a dependency-light HTML/CSS/JS interface. Primary surface: Operate; keys and connection settings come first, not marketing or fake metrics.
One-time setup requires a cryptographically random token saved privately on the server. Passwords use salted scrypt. Random opaque sessions are memory-only and expire after 12 hours, with HttpOnly SameSite=Strict cookies and Secure under HTTPS. Writes require exact configured Origin and CSRF. Persist only API-key hashes and metadata in an atomically replaced private JSON file. Run one Node process per data volume.
Production startup requires PUBLIC_URL=https://...; bind upstream to loopback and proxy with automatic TLS. No secrets in source, images, logs or browser storage. Inference keys cannot manage accounts or create keys. Browser session cookies cannot act as inference keys.

## Non-goals
No multiuser registration, payments, quota bypass, model entitlement expansion, or new provider credential flows. Existing protected account file is used read-only for qualification; no remote credential transfer without approval.
