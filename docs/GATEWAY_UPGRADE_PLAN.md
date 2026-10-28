# Antigravity API Gateway: High-Concurrency & Reliability Upgrade Plan

**Document Version:** 1.0.0  
**Status:** Planning Complete (Implementation Not Started)  
**Target Repository:** `D:/Side-Projects/antigravity-api-gateway`  
**Associated Backlog:** `docs/GATEWAY_IMPLEMENTATION_BACKLOG.json`

---

## 1. Executive Summary & Architecture Strategy

The objective of this upgrade is to transform the Antigravity API Gateway into an industrial-strength, high-throughput local and VPS-ready inference gateway. The gateway coordinates multiple Google CloudCode OAuth accounts, providing an OpenAI- and Anthropic-compatible `/v1` endpoint designed to support parallel execution from autonomous coding agents (Hermes Agent, OpenCode, and remote tools).

### Core Architectural Decisions:
1. **In-Gateway Scheduling over External Proxies:**
   External load balancers (NGINX, HAProxy, AWS ALB) cannot inspect Google OAuth bearer tokens, model rate-limit reset headers, or session cache signatures. Account balancing and queueing must reside natively within `src/account-manager/` and `src/scheduler/`.
2. **Correctness Before Concurrency:**
   Synthetic isolated testing demonstrated critical defects in the current code: stream replay after downstream emission, incorrect tool finish reasons (`stop` instead of `tool_calls`), and unexpired cooldown erasure. These must be resolved before raising concurrency.
3. **Atomic Leases over Loose Counters:**
   Instead of ad-hoc increment/decrement counters, account capacity is acquired via an atomic **Attempt Lease** synchronous reservation before async operations begin, and released idempotently in `finally` blocks.
4. **Strict Opt-In Model Fidelity:**
   The gateway must never silently downgrade models (e.g. Gemini 3.8 to 2.5) without explicit client opt-in. Advertised models and reasoning parameters must match verified upstream capabilities.
5. **Enforced Private-First Deployment:**
   The token-free single-owner dashboard architecture chosen by the user must be pre-provisioned in private local storage before public container exposure to eliminate the first-visitor takeover window.

---

## 2. Gap Analysis & Source-Grounded Defect Catalog

The following issues were identified in current source code and verified via synthetic isolated test probes:

| Finding ID | Source Location | Defect Description | Required Correction |
| :--- | :--- | :--- | :--- |
| **DEF-01** | `src/cloudcode/streaming-handler.js:101-158` | Streaming handler retry catches wrap `yield* streamSSEResponse`, causing retries after partial yields, mixing two completions into one response. | Track downstream commitment; abort with terminal error if failure occurs after first yield. |
| **DEF-02** | `src/cloudcode/sse-streamer.js:158-219`, `src/format/openai-compat.js:369-388` | Upstream Google `finishReason: STOP` overwrites `tool_use` stop reason, emitting `finish_reason: "stop"` instead of `"tool_calls"`. | Keep `stopReason = "tool_use"` when tool calls are present; map to `finish_reason: "tool_calls"`. |
| **DEF-03** | `src/cloudcode/sse-streamer.js:31-50`, `229-287` | Reader splits raw chunks by newline and discards pending buffers at EOF, converting truncated output into clean stop with empty text. | Framing-aware SSE parser with UTF-8 chunk buffering; fail closed on truncated EOF. |
| **DEF-04** | `src/server.js:776-782`, `src/constants.js:96-110` | Chat route checks rate limits against incoming alias (`gemini-3.8-flash`) before conversion resolves it to canonical (`gemini-3.8-flash-tiered`). | Canonicalize model ID at route entry; share canonical ID across admission, limits, and metrics. |
| **DEF-05** | `src/server.js:631, 776, 860`, `src/account-manager/rate-limits.js:94` | Route handlers call `resetAllRateLimits()` when all accounts are cooling, erasing valid unexpired cooldowns. | Remove blanket resets on request paths; preserve cooldowns and rely on timer expirations. |
| **DEF-06** | `src/account-manager/selection.js:19-29`, `src/account-manager/rate-limits.js:38-50` | `isAccountUsable()` only checks `isInvalid` and rate limits; accounts marked `enabled: false` remain eligible. | Explicitly verify `account.enabled !== false` in all selection and availability predicates. |
| **DEF-07** | `src/account-manager/credentials.js:29-79` | `getTokenForAccount()` lacks single-flight deduplication; concurrent requests trigger duplicate OAuth refreshes. | Share a single pending Promise for active token refreshes per account. |
| **DEF-08** | `src/cloudcode/message-handler.js:94-125`, `src/cloudcode/streaming-handler.js:93-121` | Handler catches 401, clears token cache, but reuses the same local token variable for the second endpoint attempt. | Refresh and re-assign the active token variable upon catching 401 before next attempt. |
| **DEF-09** | `src/cloudcode/rate-limit-parser.js:79-86` | Regex for `quotaResetDelay` has over-escaped digits (`\\d+`), causing decimal durations (`1.5s`) to fail parsing. | Correct regex patterns; validate finite nonnegative integer and float durations. |
| **DEF-10** | `src/account-manager/storage.js:124-163`, `src/account-manager/index.js:252` | `saveAccounts()` is unawaited in selection; overlapping concurrent saves can result in race conditions. | Queue save operations sequentially via an asynchronous write chain. |

---

## 3. High-Concurrency Scheduler Architecture

```
Incoming Request (Hermes / OpenCode)
               │
               ▼
┌───────────────────────────────────────────────┐
│          1. Route Entry & Validation          │
│   - Check API Key Auth & Body Size            │
│   - Canonicalize Model (resolveModel)         │
│   - Attach AbortSignal / Timeout Deadline     │
└──────────────────────┬────────────────────────┘
                       │
                       ▼
┌───────────────────────────────────────────────┐
│         2. Bounded Admission Queue            │
│   - Check Queue Depth (Max Requests)          │
│   - Check Queue Bytes (Max Memory Payload)    │
│   - Fair Queuing by API Key                   │
│   - Prune Immediately on Client Disconnect    │
└──────────────────────┬────────────────────────┘
                       │ (When slot available)
                       ▼
┌───────────────────────────────────────────────┐
│     3. Atomic Attempt Lease Engine            │
│   - Synchronous Select-and-Reserve            │
│   - Filter: enabled, valid, not rate-limited  │
│   - Balance: Least In-Flight (active leases)  │
│   - Tie-Break: Least-Recently-Used            │
│   - Increment inFlightCount; return Lease     │
└──────────────────────┬────────────────────────┘
                       │
                       ▼
┌───────────────────────────────────────────────┐
│       4. Execution & Upstream Dispatch        │
│   - Single-Flight Token Acquisition           │
│   - Upstream CloudCode Stream / JSON Call     │
│   - Endpoint Failover (Daily -> Prod)         │
│   - Downstream Commitment Enforcement         │
└──────────────────────┬────────────────────────┘
                       │ (finally)
                       ▼
┌───────────────────────────────────────────────┐
│             5. Guaranteed Cleanup             │
│   - lease.release() (Idempotent decrement)    │
│   - Wake Next Queue Waiter                    │
│   - Record Latency & Token Usage Telemetry    │
└───────────────────────────────────────────────┘
```

### 3.1 Attempt Lease Specification
- An **Attempt Lease** represents temporary execution rights on an account.
- **Reservation:** Must be granted synchronously during selection to eliminate race conditions between multiple async requests.
- **Ceilings:** Configurable per-account concurrency limit (`MAX_CONCURRENT_PER_ACCOUNT = 1` or `2`) and total gateway concurrency limit.
- **Idempotence:** Calling `lease.release()` multiple times or during abnormal aborts must decrement the counter exactly once and never below zero.

### 3.2 Bounded Admission Queue Specification
- **Capacity Limits:** Maximum queue length (e.g. 50 requests) and maximum queue payload buffer (e.g. 50 MB total buffered messages).
- **Overload Handling:** Requests arriving when the queue is full receive HTTP 429 with JSON error:
  `{"error": {"type": "insufficient_quota", "message": "Gateway overloaded. Please retry after delay."}}`
- **Cancellation:** If a client drops the connection while queued, the pending item is immediately purged, freeing slots without invoking Google APIs.

---

## 4. Work Package Decomposition & Detailed Plan

The implementation backlog is organized into four sequential milestones:

### Milestone 1: Protocol & Stream Reliability Core

### WP-01 Stream Replay Elimination & Downstream Commitment Guard
- **Requirements Covered:** `REQ-STRM-REPLAY`
- **Goal:** Prevent stream handlers from restarting or switching endpoints once partial output has been yielded to the client.
- **File Ownership:** `src/cloudcode/streaming-handler.js`, `tests/unit/streaming-handler.test.mjs`
- **Implementation:**
  - Introduce `let downstreamCommitted = false;` in `sendMessageStream`.
  - Mark `downstreamCommitted = true` immediately before yielding the first SSE event.
  - Wrap downstream yield in a dedicated try block: if an error occurs after commitment, emit a terminal error event and return immediately without looping to subsequent endpoints.
- **Tests & Evidence:** Test simulating network disruption after 1 chunk; verify stream terminates with error event and no duplicate `message_start`.

### WP-02 Streamed Tool Calls Finish Reason Alignment
- **Requirements Covered:** `REQ-STRM-TOOLS`
- **Goal:** Ensure functionCall blocks in streaming responses emit finish_reason `'tool_calls'` rather than `'stop'`.
- **File Ownership:** `src/cloudcode/sse-streamer.js`, `src/format/openai-compat.js`, `tests/unit/sse-tool-finish.test.mjs`
- **Implementation:**
  - In `sse-streamer.js`, preserve `stopReason = 'tool_use'` even if upstream `finishReason` is `STOP`.
  - In `openai-compat.js` (`convertAnthropicEventToOpenAI`), map `message_delta` with `stop_reason: 'tool_use'` to OpenAI `finish_reason: 'tool_calls'`.
- **Tests & Evidence:** Test streaming tool call candidates; verify `choices[0].finish_reason === 'tool_calls'`.

### WP-03 Framing-Aware SSE Parser & Truncation Guard
- **Requirements Covered:** `REQ-STRM-FRAMING`
- **Goal:** Implement robust SSE framing parser that handles chunk boundaries, UTF-8 splits, and rejects truncated EOFs.
- **File Ownership:** `src/cloudcode/sse-streamer.js`, `src/cloudcode/sse-parser.js`, `tests/unit/sse-framing.test.mjs`
- **Implementation:**
  - Buffer multi-byte UTF-8 sequences across stream chunks using `TextDecoderStream` or persistent buffer.
  - Fail closed with `StreamTruncatedError` if stream terminates without proper terminating newline or finish reason.
- **Tests & Evidence:** Tests validating fragmented JSON lines, split UTF-8 emojis, and abrupt EOF conditions.

### WP-04 Canonical Model Identity & Cooldown Scoping
- **Requirements Covered:** `REQ-MODEL-CANONICAL`
- **Goal:** Ensure all routes, rate-limit checkers, and request converters use canonical model IDs consistently.
- **File Ownership:** `src/server.js`, `src/constants.js`, `src/account-manager/rate-limits.js`, `tests/unit/model-canonical.test.mjs`
- **Implementation:**
  - Apply `resolveModel(req.body.model)` at route entry in `/v1/chat/completions`, `/v1/responses`, and `/v1/messages`.
  - Key all `modelRateLimits` dictionaries exclusively by the resolved canonical model string.
- **Tests & Evidence:** Test registering cooldown on `gemini-3.8-flash-tiered` and asserting `isAllRateLimited('gemini-3.8-flash')` evaluates to `true`.

### WP-05 Strict Account Eligibility & Route Cooldown Preservation
- **Requirements Covered:** `REQ-ACCT-ELIGIBILITY`
- **Goal:** Prevent selection of disabled accounts and remove reckless blanket rate-limit resets in route handlers.
- **File Ownership:** `src/account-manager/selection.js`, `src/account-manager/rate-limits.js`, `src/server.js`, `tests/unit/eligibility.test.mjs`
- **Implementation:**
  - Update `isAccountUsable()` and `getAvailableAccounts()` to explicitly check `account.enabled !== false`.
  - Remove calls to `resetAllRateLimits()` in `src/server.js` lines 631, 776, 860.
- **Tests & Evidence:** Test account with `enabled: false` is never selected; test unexpired rate limits are preserved when new requests arrive.

---

### Milestone 2: Auth Concurrency & Storage Integrity

### WP-06 Single-Flight Token Refresh & Post-401 Re-acquisition
- **Requirements Covered:** `REQ-AUTH-SINGLEFLIGHT`
- **Goal:** Deduplicate concurrent refresh requests per account and refresh token prior to second-endpoint retry.
- **File Ownership:** `src/account-manager/credentials.js`, `src/cloudcode/message-handler.js`, `src/cloudcode/streaming-handler.js`, `tests/unit/auth-singleflight.test.mjs`
- **Implementation:**
  - Track in-flight refresh promises in a `Map<email, Promise<string>>`.
  - In message handlers, ensure catching 401 triggers an immediate token refresh and updates the local `token` variable before attempting the fallback endpoint.
- **Tests & Evidence:** Test spawning 5 concurrent token requests for one account; verify exactly one OAuth refresh request is sent.

### WP-07 Quota Reset Delay & Error Duration Parser Hardening
- **Requirements Covered:** `REQ-PARSER-COOLDOWN`
- **Goal:** Fix regexes and parsing logic for quotaResetDelay, Retry-After, and ISO timestamps.
- **File Ownership:** `src/cloudcode/rate-limit-parser.js`, `tests/unit/rate-limit-parser.test.mjs`
- **Implementation:**
  - Correct regex in `parseResetTime` from `([\\d\\.]+)` to `/quotaResetDelay[:\s"]+([\d.]+)(ms|s)/i`.
  - Parse milliseconds and decimal seconds accurately into integer milliseconds.
- **Tests & Evidence:** Test payloads with `"quotaResetDelay": "1.5s"` -> 1500ms and `"754.43ms"` -> 755ms.

### WP-08 Serialized Account State Persistence & Dynamic Reload
- **Requirements Covered:** `REQ-STATE-CONCURRENCY`
- **Goal:** Serialize account writes using an in-memory queue and ensure reload endpoint completely refreshes state.
- **File Ownership:** `src/account-manager/storage.js`, `src/account-manager/index.js`, `src/server.js`, `tests/unit/storage-concurrency.test.mjs`
- **Implementation:**
  - Maintain a sequential Promise chain for `saveAccounts` invocations.
  - Implement full reload in `AccountManager.reload()` that re-reads `accounts.json` from disk and updates internal structures without server restart.
- **Tests & Evidence:** Rapid concurrent save test confirming zero file corruption; test modifying `accounts.json` and asserting `/v1/accounts/reload` reflects the change.

---

### Milestone 3: High-Concurrency Scheduler & Admission Queue

### WP-09 Atomic Account Lease Engine (Select-and-Reserve)
- **Requirements Covered:** `REQ-SCHED-LEASE`
- **Goal:** Implement in-flight connection tracking with atomic reservation and guaranteed lease release.
- **File Ownership:** `src/account-manager/lease-engine.js`, `src/account-manager/selection.js`, `src/account-manager/index.js`, `tests/unit/lease-engine.test.mjs`
- **Implementation:**
  - Create `LeaseEngine` tracking `inFlightCount` per account.
  - Selection checks capacity and synchronously increments `inFlightCount`, returning `{ account, lease }`.
  - Expose `lease.release()` with atomic latch ensuring single execution.
- **Tests & Evidence:** Concurrent test allocating 6 accounts across 6 requests; verifying in-flight count never goes negative.

### WP-10 Bounded Fair Admission Queue & Cancellation Pruning
- **Requirements Covered:** `REQ-SCHED-QUEUE`
- **Goal:** Implement an in-memory queue to absorb temporary traffic spikes with byte limits and client abort handling.
- **File Ownership:** `src/scheduler/admission-queue.js`, `src/server.js`, `tests/unit/admission-queue.test.mjs`
- **Implementation:**
  - Implement queue with FIFO order, per-key fair sharing, and max depth/byte limits.
  - Listen to `req.on('close')` to immediately remove aborted requests from queue.
- **Tests & Evidence:** Test queue saturation emitting 429; test client cancellation pruning queue immediately.

### WP-11 Telemetry & Accurate Streamed Token Usage Accounting
- **Requirements Covered:** `REQ-OBSERVABILITY`
- **Goal:** Record per-request queue delays, lease durations, and repair prompt_tokens calculation in SSE events.
- **File Ownership:** `src/format/openai-compat.js`, `src/utils/telemetry.js`, `src/server.js`, `tests/unit/telemetry.test.mjs`
- **Implementation:**
  - Extract `promptTokenCount` and `candidatesTokenCount` from Google `usageMetadata` and populate `usage` in final SSE chunks.
  - Emit structured JSON log on completion with queue time, lease time, and token counts (strictly excluding prompt content).
- **Tests & Evidence:** Test verifying streamed SSE contains non-zero `prompt_tokens` matching response metadata.

### WP-12 Capability-Aware Request Mapping & Opt-In Fallback
- **Requirements Covered:** `REQ-MODEL-CAPABILITY`
- **Goal:** Enforce strict opt-in model fallback, validate model capabilities, and fix thinking/output parameter mapping.
- **File Ownership:** `src/fallback-config.js`, `src/format/request-converter.js`, `src/format/openai-compat.js`, `tests/unit/model-capability.test.mjs`
- **Implementation:**
  - Keep fallback disabled by default (`FALLBACK_ENABLED = false`).
  - Map `reasoning_effort` and `thinkingLevel` accurately to Google `thinkingConfig.thinkingBudget`.
  - Cap Gemini output tokens to `GEMINI_MAX_OUTPUT_TOKENS` (16,384) with clear logging.
- **Tests & Evidence:** Test verifying fallback does not trigger unless enabled; test thinking configuration values across low/medium/high.

---

### Milestone 4: Empirical Qualification & Production Deployment

### WP-13 Controlled Canary Load Benchmark & Capacity Qualification
- **Requirements Covered:** `REQ-BENCHMARK-LOAD`
- **Goal:** Conduct a separately authorized, small-scale live benchmark to measure actual RPM and latency under real quotas.
- **File Ownership:** `scripts/benchmark-concurrency.mjs`, `docs/BENCHMARK_RESULTS.md`
- **Authorization:** `live-approval` (requires explicit user consent before execution).
- **Implementation:**
  - Automated test script simulating 1 to 6 concurrent agent turns with real tool calls.
  - Halts automatically upon encountering 2 consecutive 429 errors or account verification warnings.
  - Generates empirical latency and throughput report.
- **Tests & Evidence:** Execution log and markdown benchmark report documenting observed limits.

### WP-14 Client Integration & Thinking Profile Validation
- **Requirements Covered:** `REQ-CLIENT-INTEG`
- **Goal:** Validate end-to-end tool execution and reasoning stream rendering in OpenCode and Hermes.
- **File Ownership:** `tests/integration/client-compatibility.test.mjs`
- **Authorization:** `client-approval` (requires explicit user consent).
- **Implementation:**
  - Test tool execution and multi-turn continuations using Hermes CLI alias and OpenCode provider.
  - Verify reasoning content is rendered without schema errors.
- **Tests & Evidence:** Recorded transcript of clean multi-turn tool interaction.

### WP-15 Pre-Provisioned Owner Bootstrap & Packaging Hardening
- **Requirements Covered:** `REQ-DEPLOY-BOOTSTRAP`
- **Goal:** Enforce private owner setup prior to public start and reconcile start-vps.sh instructions.
- **File Ownership:** `deploy/start-vps.sh`, `deploy/install.sh`, `src/index.js`, `tests/console/deployment.test.mjs`
- **Implementation:**
  - In `start-vps.sh`, fail closed if `data/console/access.json` does not contain an established owner.
  - Remove all obsolete setup-token text from scripts and docs.
- **Tests & Evidence:** Deployment tests asserting startup refusal if unconfigured in production mode.

### WP-16 Production Caddy Reverse Proxy & Service Networking
- **Requirements Covered:** `REQ-DEPLOY-VPS`
- **Goal:** Configure production Docker Compose stack with internal Caddy service networking and unbuffered SSE.
- **File Ownership:** `deploy/Caddyfile`, `docker-compose.yml`, `Dockerfile`
- **Authorization:** `remote-approval` (requires explicit user consent).
- **Implementation:**
  - Configure Caddy reverse proxy to `antigravity-gateway:8080` with `flush_interval -1`.
  - Ensure gateway binds loopback on host while accessible to Caddy over internal Docker network.
- **Tests & Evidence:** Verified container configuration and successful HTTPS curl test.

---

## 5. Verification Gates, Rollout & Rollback Protocol

### 5.1 Verification Checklist Prior to Production
1. **Automated Unit & Regression Suite:**
   All unit tests under `tests/unit/` pass (`node --test tests/unit/*.test.mjs`).
2. **Console & Auth Security Suite:**
   All 14 tests in `tests/console/*.test.mjs` pass with zero regressions.
3. **Stream Parity & Lifecycle Invariants:**
   Simulated network drops, tool calls, and truncated streams produce expected protocol behavior. Handlers enforce separate HTTP commitment and semantic token yield, ensuring no replay loops or invalid placeholders.
4. **Pre-Provisioning Verification:**
   Owner account verified present in `data/console/access.json` before any public DNS exposure. Both `start-vps.sh` and runtime startup in `src/index.js` enforce fail-closed checks when running with public bindings.

### 5.2 Safe Rollout Sequence & Dependency Gates
1. **Milestone M1 (Protocol & Stream Reliability Core):** Implement WP-01, WP-02, WP-03, WP-04, and WP-05. Verify zero stream replays, accurate tool finish reasons, framing-aware SSE parsing, and canonical model routing.
2. **Milestone M2 (Auth Concurrency & Storage Integrity):** Implement WP-06, WP-07, and WP-08. Verify single-flight OAuth refresh under delayed 401s, accurate quota delay parsing, and sequential state saves with reload coordination.
3. **Milestone M3 (High-Concurrency Scheduler & Admission Queue):** Implement WP-09, WP-10, WP-11, and WP-12. Verify attempt leases, fair queueing with cancellation pruning, downstream backpressure handling, accurate token accounting, and capability-aware request mapping.
4. **Milestone M4 (Empirical Qualification & Production Deployment):** With explicit user authorization:
   - Execute WP-13 (Canary benchmark with hard safety ceilings).
   - Execute WP-14 (Client integration tests with Hermes and OpenCode).
   - Execute WP-15 and WP-16 (Pre-provisioned owner verification and Caddy Docker stack deployment).

### 5.3 Coherent Rollback & Recovery Protocol
- In the event of scheduler regression, queue stalls, or upstream auth failures in production:
  1. Do not apply isolated single-file reverts that decouple selection from the lease engine and queue.
  2. The scheduler and admission queue must support a unified runtime feature toggle (`SCHEDULER_MODE=legacy` or git-level deployment revert of the complete M3 package).
  3. When rolling back, gracefully drain or cancel active requests, stop incoming admissions, and restart the gateway service cleanly.
  4. Never overwrite `data/accounts.json` with an unverified pre-deployment snapshot without reconciling active credentials, new accounts, and unexpired cooldowns.
  5. Security boundaries (single-owner auth, CSRF guards, and path traversal defenses) are strictly preserved across any rollback.

---

## 6. Independent Plan Critique & Disposition Matrix

An independent review of this plan was conducted using Codex CLI (`gpt-6.1-sol`) in a read-only sandbox. All 14 findings have been evaluated and addressed as follows:

| Critique ID | Severity | Core Finding | Disposition & Concrete Resolution |
| :--- | :--- | :--- | :--- |
| **CRIT-01** | High | Lease engine integration into actual streaming/non-streaming dispatch loops is unassigned; lacks attempt lifecycle definition. | **Accepted:** Explicitly assigned dispatch loop lease ownership to WP-09 and `src/server.js`. Leases must be acquired synchronously before upstream async dispatch and released in `finally` across all three API routes. |
| **CRIT-02** | High | `req.on('close')` can fire prematurely upon POST body receipt rather than response cancellation. | **Accepted:** Updated WP-10 to use `res.on('close')` and an internal `AbortSignal` tied to the client socket, ensuring queued requests are not misclassified as aborted. |
| **CRIT-03** | High | Architecture lacks explicit end-to-end deadline policies and cooldown-expiry queue wakeups. | **Accepted:** Added explicit timeout policies (queue wait, connection, stream idle) and scheduled timer wakeups on earliest cooldown expiry to WP-09 and WP-10. |
| **CRIT-04** | High | Queue byte limits do not bound memory consumed by active streams; lacks downstream backpressure handling. | **Accepted:** Enhanced WP-10 and route handlers to check `res.write()` return value and pause upstream SSE consumption until the `'drain'` event fires. |
| **CRIT-05** | High | Concurrency race between state reload, queued saves, in-flight token refreshes, and active leases. | **Accepted:** Assigned configuration generation tracking and coordinator serialization to WP-08 and WP-09, ensuring active leases and refreshes survive dynamic reloads. |
| **CRIT-06** | Medium | Single-flight refresh test lacks delayed staggered 401 scenarios and cache poisoning protection. | **Accepted:** Updated WP-06 acceptance criteria to require conditional cache invalidation based on token generation and guaranteed cleanup in `finally` blocks. |
| **CRIT-07** | High | Rollback protocol proposed reverting only `selection.js`, which would break coupled queue/lease mechanisms. | **Accepted:** Redesigned §5.3 to require atomic, whole-scheduler feature toggling or unified git rollback, preventing decoupled orphan queue waiters. |
| **CRIT-08** | High | Owner check in `start-vps.sh` could be bypassed by direct `docker compose up`. | **Accepted:** Updated WP-15 to enforce a pre-listen schema validation check inside `src/index.js` when running in production or public binding modes. |
| **CRIT-09** | Medium | Backlog dependency graph allowed WP-09/WP-10 before stream correctness and persistence hardening were complete. | **Accepted:** Updated `GATEWAY_IMPLEMENTATION_BACKLOG.json` dependencies so M3 packages strictly depend on M1 and M2 completion. |
| **CRIT-10** | Medium | Universal thinking mapping and 16,384 Gemini output cap lacked verified per-model capability specifications. | **Accepted:** Updated WP-12 to establish a versioned model capability registry with verified thinking level ranges and output token limits. |
| **CRIT-11** | Medium | Ambiguity between HTTP header flush commitment and semantic stream yield in WP-01. | **Accepted:** Updated WP-01 to distinguish HTTP response header commitment from semantic SSE token emission, preventing replay after either boundary. |
| **CRIT-12** | Medium | Conflict between FIFO ordering and per-key fair sharing in queue specification. | **Accepted:** Clarified WP-10 to enforce weighted fair queueing across distinct API keys with strict FIFO within each individual key queue. |
| **CRIT-13** | Medium | Observability acceptance criteria lacked TTFT, model identity tracking, and token accounting precision. | **Accepted:** Updated WP-11 acceptance criteria to mandate recording monotonic TTFT, requested vs canonical model ID, and exact Google prompt/candidate token metadata. |
| **CRIT-14** | Medium | Canary benchmark lacked executable hard safety ceilings and sample size rules. | **Accepted:** Updated WP-13 to enforce hard request, token, and elapsed-time caps in `scripts/benchmark-concurrency.mjs`, with automatic abort on consecutive errors. |

