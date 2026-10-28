## ADDED Requirements
### Requirement: Single owner
The website SHALL allow one owner to set up an account using a private bootstrap token and subsequently log in; it SHALL NOT expose public registration.
#### Scenario: Unauthorized setup
- **WHEN** a visitor submits setup without the correct token or tries after setup
- **THEN** access is denied and owner credentials remain unchanged.
### Requirement: API key lifecycle
The owner SHALL create, name, copy once, list and revoke persistent API keys from the website.
#### Scenario: Key is revoked
- **WHEN** the owner revokes a generated key
- **THEN** the key immediately fails inference authentication and never grants management access.
### Requirement: Safe deployment
The service SHALL fail closed and protect admin credentials and account details when deployed behind HTTPS.
#### Scenario: Public visitor
- **WHEN** a visitor has no owner session or API key
- **THEN** only login assets and minimal liveness are available, not account details or inference.
