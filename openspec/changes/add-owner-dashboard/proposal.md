# Change: Private owner console

## Why
The owner requested an AI Studio-like website on a VPS: log in privately, generate an API key and use it from existing clients.

## What Changes
- Add single-owner, token-protected first setup and password login, with no public registration.
- Add persistent hashed API keys, one-time reveal, copyable connection details, revocation and model listing.
- Integrate generated keys with existing inference; separate admin sessions from application keys.
- Fail closed without credentials and remove account details from public health; reject remote local-file access.
- Provide HTTPS reverse proxy deployment with persistent private state.

## Impact
- Existing API endpoints remain; legacy environment keys authorize inference only.
- Local implementation scope is authorized by the current user request. Remote publication/deployment is not authorized without a target and confirmation.
- Provider quota, model entitlement and third-party service terms remain applicable. A consumer Pro subscription is not unlimited official API credit.
