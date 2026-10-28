# Complete review: Antigravity API Gateway scaling plan

## 1. Scope, evidence and verdict

Verdict: revise before implementation. Keep account-aware scheduling inside the gateway, but fix stream/retry correctness and quota handling before increasing concurrency. Do not enable silent model downgrades or promise throughput from an account count.

This reviews the entire four-phase proposal, including its roadmap and earlier capacity explanations. It is not authorization to implement, change client settings, onboard accounts, restart services or deploy. Only this report and review evidence/scripts were created. The working tree was already dirty; HEAD alone is not the reviewed implementation. The source snapshot is anchored at HEAD `4f105edf0994838d81a69d2ab372123462235ce0` plus the current worktree bytes recorded in `.unlazy/scaling-plan-review/baseline.json`.

Evidence vocabulary:

- VERIFIED FACT (source): directly inspected current code, not a production outcome.
- VERIFIED FACT (probe): observed with isolated synthetic fixtures and stubbed fetch, not live provider capacity.
- INFERENCE: consequence suggested by the source; not directly demonstrated in production.
- PROPOSAL: a recommended future contract, not implemented behavior.
- UNKNOWN: requires provider evidence or separately authorized operational tests.

Twelve isolated characterization cases reproduced existing behavior. Passing these assertions confirms the observations, including defects; it does not mean the gateway passed twelve production acceptance tests. Existing console regression tests were also run with disposable state. No live Google/CloudCode inference calls or provider-load tests were performed for this review; the independent Codex analysis below is a separate model-assisted review.

An independent read-only Codex review examined a numbered source packet, not the entire repository, and returned a substantive structured verdict. Its cited source bytes were hash-matched afterward. Its reliance on the parent's probe results is not independent reproduction. The first native reviewer produced no actual review and is excluded as evidence.

PRIVATE_STATE_DRIFT: hashes of `data/accounts.json` and `data/console/access.json` changed during review while an existing gateway process was serving on loopback. Selection/state saving and key last-used tracking contain normal request-side writes (`src/account-manager/selection.js:102-105`, `src/account-manager/index.js:252-253`, `src/account-manager/storage.js:124-163`, `src/console/access.mjs:87-96`). Exact per-write process attribution is not established. Runtime activity is a plausible explanation, not proven attribution. Review scripts do not write those private paths. No claim is made that private-state bytes remained identical. Do not stop the user's server or overwrite its data to manufacture a clean comparison.

## 2. Review of every original phase and roadmap item

| Original item | Disposition | Required correction |
| --- | --- | --- |
| Internal least-connections account routing | KEEP, REDESIGN | Use atomic attempt leases, explicit capacity, eligibility and cancellation, not two loose counter updates. |
| Increment at request start; decrement on completion or `[DONE]` | REPLACE | Acquire before async work; release exactly once on every exit. `[DONE]` is an API framing marker, not the resource owner. |
| Least in-flight then least-recently-used | KEEP AS INITIAL POLICY | Make tie-breaking deterministic; use normalized occupancy if capacities differ. Health is eligibility/cooldown first, not an unexplained score. |
| Six incoming requests always use six accounts without waiting | QUALIFY | Only a synthetic scheduler property with six eligible, equally configured accounts and one slot each; not a live quota or latency guarantee. |
| Promise queue with 15-second wait | KEEP, BOUND | Specify queue length, fairness, per-key limits, cancellation, total deadline and wakeups. A queue does not create capacity. |
| Enable model fallback by default | REJECT | Preserve same-model behavior by default. Require explicit allowed fallback policy. |
| Gemini 3.8 -> 3.7 -> 3.5 fallback chain | REPLACE WITH VERIFIED CAPABILITIES | This chain is absent from current fallback mappings; advertised aliases do not prove upstream model availability. |
| Independent model quotas; no agent will halt | RETRACT | Quota independence is unknown; no finite pool or fallback eliminates provider exhaustion. |
| Daily/Prod cycling on 429 | ALREADY PARTLY PRESENT | Repair structured error aggregation and cooldown semantics. Do not describe endpoints as independent quota pools. |
| Raise maximum accounts from 10 to 30 | DEFER | Inventory cap is not request capacity. Fix admission/retry behavior and justify a configurable inventory limit from evidence. |
| New CLI onboarding; under 30 seconds/account | RECONCILE, NO TIME PROMISE | Add/import commands already exist. OAuth, MFA, user consent and provider checks determine onboarding duration. |
| Worker minimal/low, thinker medium/high | CONDITIONAL | Validate actual client payload and upstream translation. Evaluate task quality, not just response speed. |
| Worker turns take 1.5-2 seconds and free capacity 2-3 times faster | RETRACT | No controlled workload benchmark supports these figures. |
| Workers on Flash, heavy reasoning on a separate model | OPTIONAL CLIENT POLICY | Can be useful but changes client orchestration; it is not an automatic gateway throughput feature. Pro access is not unlimited/free API credit. |
| Dockerfile, Compose and private data volume | ALREADY PRESENT | Reconcile and harden existing assets rather than recreating them. |
| Loopback gateway port and Caddy HTTPS | KEEP, CORRECT TOPOLOGY | Host port may bind loopback; a Caddy container must use the gateway service name and container port. |
| Caddy streaming flush | ALREADY PRESENT | Test incremental delivery, disconnects and slow readers. Flush settings alone do not prove end-to-end streaming. |
| Owner password and hashed inference keys | KEEP | Separate owner-only management from inference, and privately provision the first owner before any public exposure. |
| Roadmap: true parallel execution | QUALIFY | Node already accepts overlapping async requests. New scheduling improves distribution/admission, not the existence of parallelism. |
| Roadmap: zero rate-limit stoppage | REJECT | The objective is bounded degradation and honest errors, not unlimited upstream capacity. |
| Roadmap: ready for 100+ requests/min | RETRACT | Must be measured for a defined workload and quota entitlement. |
| Roadmap: anywhere with a single API key | CORRECT | Clients still need the HTTPS base URL and model, plus network reachability and key. |

## 3. Source-grounded defects that precede scaling

### P1: Streaming replay can combine different answers

VERIFIED FACT (source/probe): the endpoint catch surrounds `yield* streamSSEResponse`, so failure after yielding output can continue into another endpoint. A synthetic interrupted stream produced two message starts and text from both the partial first answer and a second completion (`src/cloudcode/streaming-handler.js:101-158`). The HTTP route forwards those events into the same client response (`src/server.js:785-809`).

INFERENCE: higher concurrency and more retries increase exposure to corrupted response framing and potentially repeated tool instructions. This does not prove a tool was executed twice in a real client.

Required contract: distinguish HTTP commitment from model-output commitment. Perform admission before flushing SSE headers. Transparent endpoint/account/model retry is allowed only before any model event is exposed; after exposure, fail the same stream without replay. A role-only event also commits protocol state unless deliberately buffered. Never fabricate a successful terminal event for an interrupted generation.

### P1: Streamed tool calls can finish as ordinary text

VERIFIED FACT (source/probe): a function call sets `tool_use`, but upstream `finishReason: STOP` overwrites it with `end_turn` (`src/cloudcode/sse-streamer.js:158-219`). OpenAI conversion then reports `stop`, not `tool_calls` (`src/format/openai-compat.js:369-388`). The synthetic tool fixture reproduces this. Non-stream Chat Completions has an explicit finish-reason override (`src/server.js:811-815`); that does not fix the streaming path.

Required contract: when a completed OpenAI response contains tool calls, its finish reason must reflect tool calls. Preserve IDs, arguments, tool signatures and multi-tool indices through the subsequent tool-result turn. Test all API adapters, not just text output.

### P1: Truncated or empty SSE can be presented as success

VERIFIED FACT (source/probe): the reader processes newline-separated data and drops its pending buffer at EOF. With an unterminated final data line, it emitted `[No response received from API]` and a clean stop (`src/cloudcode/sse-streamer.js:31-50`, `src/cloudcode/sse-streamer.js:229-287`).

Important qualification: an unterminated final line is an incomplete SSE frame, not necessarily a valid event that the SSE specification requires dispatching. The defect relevant to agents is reporting successful generation for an empty/truncated response, not merely discarding incomplete framing. Do not blindly parse partial JSON as a valid event.

Required contract: framing-aware parsing, UTF-8 decoder flush, defined EOF handling, explicit upstream completion evidence and a failure on unexpected truncation. Cover complete event boundaries, comments, CRLF, split JSON/code points, malformed JSON, multiline data and finish metadata. Ensure reader cancellation and lock release in cleanup.

### P1: Cooldowns are bypassed or erased

VERIFIED FACT (source/probe): rate-limit maps are keyed by the exact model string. Chat admission checks the incoming alias before conversion resolves it, so alias and canonical keys can disagree (`src/server.js:776-782`, `src/format/openai-compat.js:131-135`, `src/constants.js:96-110`). Ordinary inference routes explicitly clear all model cooldowns when the selected model appears exhausted (`src/server.js:631-637`, `src/server.js:776-780`, `src/server.js:860-864`, `src/account-manager/rate-limits.js:94-103`). An isolated fixture reproduced the mismatch and clearing of an unexpired cooldown.

Required contract: validate and resolve the model once at entry. Retain requested ID separately from canonical effective ID. Share canonical identity across admission, quota bookkeeping, upstream request, fallback lookup and telemetry. Do not erase unexpired cooldowns on request arrival. Unknown/shared quota scope must be represented conservatively; a per-model map is not proof of an independent provider bucket.

### P1: Disabled accounts remain eligible

VERIFIED FACT (source/probe): both selection and availability exclude invalid accounts and active model cooldowns, but not `enabled:false` (`src/account-manager/selection.js:19-29`, `src/account-manager/rate-limits.js:38-50`). Storage preserves the enabled flag (`src/account-manager/storage.js:131-145`). A disabled synthetic account was selected.

Required contract: a common eligibility predicate must exclude administratively disabled accounts, invalid credentials and unavailable/unsupported model entitlements. Separate unconfigured, disabled, invalid, busy and cooling states. Define whether disable affects only new leases or also cancels active ones; do not silently change active-request semantics.

### P1: Authentication refresh is duplicated and retry uses stale credentials

VERIFIED FACT (source/probe): refresh caches completed tokens but has no shared pending-refresh promise (`src/account-manager/credentials.js:29-79`). Four concurrent fixture calls caused four refresh fetches. After a 401, handlers clear caches but keep using a token acquired outside the endpoint loop; the second endpoint reused that same token (`src/cloudcode/message-handler.js:94-125`, `src/cloudcode/streaming-handler.js:93-121`).

Required contract: per-account single-flight token/project acquisition, one bounded reacquisition after an upstream authentication failure, and structured classification of terminal invalid credentials versus transient network/provider failures. Do not invalidate an account for every transient OAuth server error. A caller's cancellation should stop that caller waiting without cancelling a shared refresh needed by other callers. Always release its account lease.

### P1: Reset parsing and endpoint error classification are unreliable

VERIFIED FACT (source/probe): the quota-delay regex has escaped digit patterns that do not match the documented durations (`src/cloudcode/rate-limit-parser.js:79-86`). The fixture `1.5s` became 5000 ms via a later fallback parser; a millisecond duration returned null, while an integer Retry-After header worked. This is not a trustworthy reset oracle.

VERIFIED FACT (source): a mutable `lastError` mixes 429, authentication, server and network results across endpoints. Its final value does not establish that all endpoints returned 429; minimum reset selection also assumes more than the evidence supplies (`src/cloudcode/message-handler.js:102-177`, `src/cloudcode/streaming-handler.js:101-169`).

Required contract: preserve structured outcomes per attempt and quota scope. Parse provider detail objects before heuristic text. Validate finite nonnegative durations, units, timestamps and Retry-After. Use bounded jittered backoff when reset information is absent. Never select the shortest advertised reset across endpoints without knowing whether the quota scopes are shared or separate.

### P2: Provider exhaustion is converted into a client input error

VERIFIED FACT (source): error mapping deliberately turns provider 429/quota exhaustion into HTTP 400 `invalid_request_error` to suppress client retries (`src/server.js:102-128`). That conflates valid requests blocked by quota with invalid client input. Streaming routes flush headers before admission, so later exhaustion can only be represented inside SSE (`src/server.js:785-809`, `src/server.js:869-894`).

PROPOSAL: define explicit overload/error contracts across Chat Completions, Responses and Messages. Use 400 for invalid input, 401/403 for appropriate caller authentication/authorization failures, 429 for defined rate/quota exhaustion with meaningful `Retry-After`, and 503 for defined temporary service unavailability/local admission overload. Do not expose upstream account-authentication failures as if the caller's gateway key were invalid. If a particular client needs a nonstandard status to avoid a retry storm, make that a documented opt-in compatibility policy rather than the universal API contract. Combine this with request-wide retry budgets and admission before SSE headers; do not merely change 400 to 429 and unleash unbounded client-plus-gateway retries.

### P2: Existing sticky routing is global, not per agent

VERIFIED FACT (source/probe): AccountManager stores one `currentIndex`, not a conversation-to-account map (`src/account-manager/index.js:33-42`, `src/account-manager/index.js:163-175`). With unchanged eligibility, six synthetic selection calls chose account zero each time (`src/account-manager/selection.js:86-106`, `src/account-manager/selection.js:166-197`). There is no busy-account reservation in those paths.

This does not show that requests execute serially. Existing async upstream requests can overlap on the same account. Comments about cache continuity do not prove a Google prompt-cache hit or savings. The session ID hashes the first user's text; separate conversations with the same opening text get the same ID (`src/cloudcode/session-manager.js:19-46`). That is an affinity collision risk, not demonstrated cross-user data disclosure.

### P2: Inventory growth also expands retry amplification

VERIFIED FACT (source): handlers set outer attempts to the larger of configured retries and account count plus one, then loop over endpoints (`src/cloudcode/message-handler.js:40-45`, `src/cloudcode/message-handler.js:102-110`, `src/cloudcode/streaming-handler.js:38-43`, `src/cloudcode/streaming-handler.js:101-107`). More accounts can increase attempts and elapsed time during failures rather than usable throughput.

Required contract: one request-wide attempt budget and monotonic deadline spanning endpoint changes, account changes, refresh and allowed model fallback. These are independent from the supported account inventory size.

### P2: State persistence and reload semantics are not concurrency-safe contracts

VERIFIED FACT (source): selection triggers unawaited saves; account writes use temporary-file replacement but have no serial writer, and errors are logged rather than propagated (`src/account-manager/selection.js:61-68`, `src/account-manager/index.js:252-253`, `src/account-manager/storage.js:124-163`). Atomic replacement prevents partial JSON; it does not prevent stale snapshots winning or coordinate external CLI writers.

VERIFIED FACT (source): the reload route resets server initialization flags but calls an AccountManager that has its own initialized guard (`src/server.js:273-283`, `src/account-manager/index.js:51-58`). Do not assume the advertised reload actually re-reads account state. Loading also resets persisted invalid flags on startup (`src/account-manager/storage.js:50-58`).

PROPOSAL: serialize/coalesce writes, surface persistence health, retain explicit operator-disabled state, and define revalidation rather than automatic invalid-account resurrection. Implement tested versioned reload or an intentional drain/restart workflow. Keep volatile leases out of durable account JSON. Use stable account identities so removal/reload cannot mis-release a lease to another array index.

## 4. Corrected scheduler and request architecture

PROPOSAL, single process initially:

Client -> authentication/validation/body limits -> canonical model and policy -> bounded admission -> atomic account lease -> bounded upstream execution -> response adapter -> cleanup.

Caddy is HTTPS ingress, not the Google-account scheduler. A separate NGINX/HAProxy layer will not inherently inspect the OAuth pool. Neither Kubernetes, Redis nor multiple gateway replicas is justified as an initial requirement.

The lease contract:

1. Check cancellation and request-wide deadline.
2. Evaluate eligibility and account-global plus model-specific capacity.
3. Select and reserve in one synchronous critical section, before credential awaits. No separate asynchronous pick-then-increment gap.
4. Return an opaque lease containing a stable account ID and an idempotent release operation.
5. Release in `finally` for successful responses, exceptions, exhausted retries, generator return, client disconnect and shutdown. Releasing twice must not make counts negative.
6. Retain the lease while that upstream attempt is consuming resources. Release it before waiting for a different account or a long cooldown. Charge endpoint/refresh retries to the common attempt/deadline budget; do not count a logical request as multiple simultaneously active account attempts unless explicitly designed.

Scheduling policy:

- Start with a configurable per-account ceiling and a global ceiling; the initial values are conservative configuration choices, not measured Google limits.
- Use least in-flight for equally configured accounts, deterministic least-recently-used tie-breaking, and optional normalized occupancy when capacities differ.
- Add retry pressure, recent latency or health weighting only after telemetry proves a need. Avoid an unvalidated complex health score.
- An account being active is not automatically full. Define capacity rather than equating one request to the entire account's allowance.
- Session affinity is optional and bounded: namespace it by authenticated key and explicit conversation ID, give it a TTL and size limit, and fall back when the account is ineligible or full. Do not promise provider cache benefits without measurement.
- Model-specific capacity must not bypass an account-wide cap if multiple models share resources. A concurrency cap alone is not a requests-per-minute or tokens-per-minute limiter; add pacing based on observed/documented limits rather than fabricated quotas.

Queue policy:

- Bound both entries and retained request bytes; large agent histories make an entry count alone inadequate.
- Separate queue timeout, credential/upstream deadlines and stream idle/total deadlines. A configurable 15-second queue wait is a product choice, not a provider-derived optimum.
- Use fair per-key admission with a global bound so one agent/client cannot monopolize the queue. Prevent starvation between models.
- Remove cancelled/expired waiters immediately. Wake on lease release, relevant cooldown expiry and eligible account changes, without busy polling or waking every waiter at once.
- Distinguish local overload, caller policy limits, no configured accounts and provider exhaustion. Return explicit retry guidance when meaningful. Do not hold an unlimited number of open sockets to avoid returning 429.

Execution and streaming policy:

- Thread cancellation through queue, sleep/backoff, token acquisition waits, fetch and body reader. Abort is terminal for that client, not a retry trigger.
- Handle `res.write()` backpressure and slow readers; bound buffering and abort upstream when the consumer disappears.
- Admit before starting SSE. After headers are committed, errors must use the API's stream error contract; HTTP status cannot be retroactively changed.
- After any model event is exposed, never transparently replay. Report interruption without false success and let the client decide a new request.
- Retrying before output may still consume upstream quota twice. Do not imply exactly-once provider generation or tool execution; do not add hedged duplicate requests by default.
- Centralize retry policy for streaming and non-streaming while preserving their different commitment semantics.

## 5. Model fidelity, fallback and client optimization

### Fallback must remain opt-in

VERIFIED FACT (source): runtime fallback is enabled only by `--fallback` or `FALLBACK=true` (`src/server.js:31-33`). The map contains exact legacy pairs, not the proposed three-model chain (`src/constants.js:206-214`, `src/fallback-config.js:18-29`). A recursive fallback disables further fallback; long cooldown exhaustion throws before that branch (`src/cloudcode/message-handler.js:58-90`, `src/cloudcode/streaming-handler.js:56-89`). Both the Flash 3.8 alias and canonical ID returned no fallback in the fixture.

VERIFIED FACT (source): `gemini-3.8-flash` resolves to `gemini-3.8-flash-tiered`; `gemini-3.5-flash` resolves to `gemini-2.5-flash` (`src/constants.js:96-111`). These are source mappings, not verification of provider availability. Calling the latter a literal 3.5 downgrade conceals the actual model.

PROPOSAL:

- Default: retry eligible accounts/endpoints for the requested canonical model only.
- Optional policy: explicit finite ordered allowlist of verified compatible models, capability constraints, visited-model protection, common deadline and attempt budget.
- Define exact fallback triggers: quota/availability versus invalid input/authentication. Do not downgrade for every 400/403 or tool-schema error.
- Test context/output limits, modalities, tool schemas, tool results, thought signatures and thinking controls before allowing a transition. Never reuse incompatible signed thinking content across model/account boundaries without a validated contract.
- Expose requested versus effective model and fallback reason in a documented response/header/telemetry contract. Current OpenAI routes pass the originally converted model to response conversion, which can hide a later fallback (`src/server.js:794-795`, `src/server.js:811-813`).
- Endpoint failover is already implemented and is an availability mechanism, not new quota (`src/constants.js:38-46`, `src/cloudcode/message-handler.js:128-136`).

### Thinking and output controls need real transport tests

VERIFIED FACT (source/probe): sending a raw `thinkingLevel: minimal` or `high` produces the same upstream configuration in the current Chat Completions conversion. `reasoning_effort: low` maps to budget 5000 and `high` to 20000; other values use the default 10000 for the tested Flash route (`src/format/openai-compat.js:174-185`). Gemini serialization sends `thinkingBudget`, not `thinkingLevel` (`src/format/request-converter.js:180-189`). An OpenCode menu displaying variants does not prove their SDK sends the expected field or the provider honors it.

VERIFIED FACT (source/probe): the gateway caps Gemini output at 16384 even when the client requests 65536 (`src/constants.js:85-86`, `src/format/request-converter.js:229-233`). This is the gateway's local cap, not a claim about Google's true maximum. The previously configured client limit must be reconciled rather than advertised as end-to-end support.

PROPOSAL: introduce a model capability contract for effective context/output limits, reasoning controls, modalities and tools. Reject unsupported values or explicitly disclose ignored settings. Validate client request -> adapter -> actual upstream payload with local capture fixtures, then separately validate actual provider effects. Do not change Hermes/OpenCode defaults during a plan review.

Worker/thinker separation remains optional. Prefer bounded prompts, relevant tool schemas and compact tool results before adding infrastructure. Compare lower-thinking profiles on representative coding/edit/test tasks and completion quality; a faster answer that breaks tool use or requires more turns can reduce useful throughput. No fixed 1.5-2 second latency or 2-3 times capacity improvement is supported.

## 6. Account operations, state and VPS security

### Account inventory and onboarding

VERIFIED FACT (source): `MAX_ACCOUNTS = 10` exists, and the interactive add path checks it (`src/constants.js:75-80`, `src/cli/accounts.js:368-381`). The runtime loader maps all imported accounts without enforcing that cap; the import path also does not enforce it (`src/account-manager/storage.js:41-71`, `src/cli/accounts.js:474-518`). It is not a universal ten-account runtime throttle.

VERIFIED FACT (source): `accounts:add` and import scripts already exist (`package.json:18-24`). The CLI add path calls a server-stop helper (`src/cli/accounts.js:469-472`), while import writes the target JSON directly without the hardened runtime save routine (`src/cli/accounts.js:501-515`). Review and redesign onboarding as a controlled operator workflow, not automatic capacity growth. Standardize `npm run accounts:add` or `npm run accounts -- add` documentation; do not execute onboarding in this review.

PROPOSAL: validate/deduplicate stable account identity, preserve existing tokens and metadata, use secure atomic persistence consistently, prevent concurrent external writers, and confirm ownership/permission for each account. Do not guarantee OAuth completion in under 30 seconds. Adding accounts can increase entitled resources only where provider policy and quota scope permit; do not design endpoint/account rotation to bypass provider restrictions.

### Preserve the chosen one-owner flow

VERIFIED FACT (source): owner passwords are scrypt-hashed; console-generated API keys and sessions are stored as hashes (`src/console/access.mjs:40-63`, `src/console/access.mjs:74-98`). HTTPS session cookies are HttpOnly, Secure and SameSite Strict; management mutations require session plus origin and CSRF (`src/console/router.mjs:24-43`). Generated inference keys do not grant owner management access (`src/server.js:86-97`, `src/middleware/auth.js:4-15`). The console page/static login shell is public; owner-only data/actions are protected. Not every `/v1/*` route is inference: account management under that prefix is owner-only.

Keep username/password-only creation of exactly one owner, without introducing a setup token or public registration. However, an empty store exposed publicly is claimable by the first person to complete setup (`src/console/router.mjs:54-65`, `src/console/access.mjs:40-50`). An Origin header is forgeable by a direct client and is not first-owner authorization.

PROPOSAL: enforce private provisioning before public ingress starts, preferably with a production readiness/startup check requiring an existing valid owner. Alternatively, explicitly restricted provisioning mode must never expose first setup to the public network. Preserve existing owner/key hashes and fail closed on malformed state; never reset or invent the user's credentials. Legacy environment keys remain supported but are not website-revocable (`src/middleware/auth.js:7-13`, `README.md:60-69`).

The global login throttle can be exhausted by other callers, blocking the owner (`src/console/router.mjs:44-50`, `README.md:89`). Add defense in depth without replacing it with an unbounded attacker-controlled per-IP map. Trust forwarded addresses only through explicitly trusted proxy topology. Consider private/Tailscale console access if remote administration does not need public exposure.

### Deployment is hardening, not greenfield work

VERIFIED FACT (source): existing Compose has non-root API configuration through the image, a network-disabled data initializer, loopback host publishing, persistent state, read-only filesystem and Caddy (`Dockerfile:1-17`, `docker-compose.yml:1-58`). The initializer validates entries before changing permissions and rejects links/special files (`scripts/prepare-data.mjs:4-24`). These are inspected configuration properties, not a new successful VPS execution.

Use the correct topology:

- Caddy inside Compose -> `antigravity-gateway:8080` on the container network. This is already the current Caddyfile (`deploy/Caddyfile:1-5`).
- Node binds `0.0.0.0` inside its container; any published host API port remains loopback-only (`docker-compose.yml:21-30`). Container loopback would make it unreachable from the separate Caddy container.
- Caddy on the host, if deliberately chosen instead, can target the published host loopback port. Do not mix both designs or assume local port 8089 is the container port.
- Verify DNS, firewall, certificate issuance/renewal, HTTPS redirects, cookie origin and incremental SSE through the real public endpoint. Required client configuration remains URL + key + model.

VERIFIED FACT (source): `deploy/install.sh` forwards to `start-vps.sh`; the latter starts services without checking that an owner exists and prints obsolete setup-token guidance (`deploy/install.sh:1-2`, `deploy/start-vps.sh:5-14`). Reconcile that helper with token-free private provisioning; do not read, remove or revive a stale token artifact as part of this review.

Single process per data volume remains the supported boundary (`README.md:63-69`). Multiple Node workers/replicas sharing JSON would split sessions, counters, cooldowns and refresh coordination. If horizontal scaling is later justified, design shared transactional state and distributed leases or disjoint account ownership first. Adding Caddy replicas does not solve those problems.

Operational additions: bounded request bodies and parse admission before retaining large histories, TLS/firewall hardening, no credentials/prompt bodies in logs, encrypted backups with a tested restore, restrictive filesystem permissions and Windows ACLs where relevant, persistent-state schema compatibility, owner/key recovery procedures, health/readiness separation, clock/deadline correctness and dependency/image maintenance. Public `/health` is currently liveness only (`src/server.js:78-79`, `README.md:71-76`). Existing shutdown closes the server with a ten-second forced exit but does not define queue/lease cancellation (`src/index.js:20-23`); add graceful draining and a bounded shutdown contract.

## 7. Acceptance tests, telemetry and performance qualification

### Existing review evidence

- `behavior-evidence.json`: twelve synthetic characterization cases, with fetch stubbed. Includes positive controls for invalid-account exclusion, known legacy fallback and integer Retry-After parsing. They prove the described current behavior, not that the proposed fixes exist.
- `console-regression-evidence.json`: fourteen disposable-state tests passed, zero failed, for owner creation, key persistence/revocation, session separation, CSRF, attachment rejection and real server middleware. This selected subset excludes deployment/audit tests; it is not a new live provider, browser, Docker or public-TLS qualification.
- `independent-review.json`: packet-only independent review, with source hashes recorded in `independent-source-hashes.json` and reconciled by the parent.
- `private-state-observation.json`: documented state drift; it contains no credential values.

Do not run the legacy full test runner without inspection: some scripts may hit live endpoints. Separately authorize quota-consuming benchmarks, real-account copies, transfers and remote deployment.

### Required deterministic tests before scaling

1. Atomic acquisition: six simultaneous requests with six eligible fixture accounts and one slot each reserve distinct accounts; no overbooking before an awaited token fetch resolves. Test unequal capacities, tie-breaking and account-global/model caps.
2. Lease cleanup: success, exception, timeout, pre-abort, mid-fetch abort, generator return, disconnect and shutdown release exactly once; no negative or leaked counters.
3. Admission: full queue rejects promptly; length/bytes remain bounded; queue timeout/cancellation removes the waiter; one noisy key cannot starve others. Fairness holds across models.
4. Eligibility: disabled, invalid, unsupported and cooling accounts are excluded. No accounts is distinct from all busy/cooling. Canonical alias limits agree everywhere.
5. Cooldowns: fake-clock tests preserve unexpired limits, schedule a single relevant expiry wakeup, prevent a thundering herd and correctly handle Retry-After, decimal seconds, milliseconds, timestamps, missing/negative/malformed values.
6. Retries: explicit matrices for 401, terminal auth, 403 entitlement, invalid request, 429, 5xx, network failure, cancellation and mixed endpoint outcomes. Assert shared attempt and total-time bounds independent of account count.
7. Credentials: concurrent calls for one account share one refresh; cancellation of one waiter does not disrupt others; 401 reacquires a token at most within policy; rotated credentials, transient refresh failures and cache invalidation remain consistent.
8. Streaming: first real delta arrives before upstream finishes; preserve reasoning/text/tools, UTF-8/framing, terminal state and accurate usage; detect empty/truncated/malformed streams; never mix another attempt after emitted output. Simulate slow readers and real downstream close.
9. Tools: harmless tool-call response and result continuation, parallel calls, argument chunks, correct `tool_calls` finish reason and signatures across every supported API adapter. A model catalog entry is not sufficient.
10. Fallback: default never changes model; opt-in uses only allowed compatible models, no cycles, no fallback after commitment, truthful effective-model reporting, deadline preserved across the chain.
11. State: overlapping saves and failed writes cannot acknowledge stale/unsafe state; account add/import/reload/removal and restart preserve disabled/cooldown/operator state and in-flight lease identity. Malformed state fails closed.
12. Security/deployment: no second owner, private bootstrap enforced, revoked keys rejected for new requests, keys cannot manage accounts, CSRF/cookie/origin checks, local-file rejection, Linux bind-mount permissions and restore tests. Verify queue/body/connection abuse limits.

The unterminated-SSE probe intentionally describes truncation; it is not a requirement to accept invalid framing. Convert each reproduced defect into a regression test for the desired behavior during implementation rather than retaining defect-expecting assertions as acceptance gates.

### Telemetry before tuning

Record a generated request ID, opaque key/account identity, requested/effective canonical model, queue delay, active leases, queue entries/bytes, time to first SSE event, first reasoning delta, first visible text/tool delta, total duration, outcome, attempted endpoints/accounts, refresh/retry/fallback counts, cooldown cause/reset, stream interruptions and persistence health. Avoid emails, bearer tokens, refresh tokens and prompt/response bodies in normal logs. Bound metric label cardinality and telemetry retention.

Current OpenAI streamed usage reports prompt tokens as zero and total tokens as output only (`src/format/openai-compat.js:369-388`). Repair usage accounting or mark it incomplete before using it for budget/capacity decisions. Provider token accounting and local estimates must be distinguished.

### Separately authorized benchmark protocol

Use one exact model, consistent prompts/context/tools/output limits and validated reasoning settings. Define success as usable task completion, including tool continuation, not an HTTP 200 or initial SSE role event. Record client/server timings with monotonic clocks. Separate gateway-only synthetic load from provider-constrained live tests.

Increase offered concurrency gradually within user-approved request/token/time budgets, with explicit stopping thresholds for 429/auth failures, queue delay, timeouts, error rate and account warnings. Report sample size, run duration, cold/warm conditions, successful requests per second/minute, task completion rate, retry amplification, queue time and p50/p95/p99 only where the sample supports those percentiles. Never extrapolate a handful of short requests into a sustainable limit.

Earlier timing measured the first SSE event, not necessarily the first visible token. A role-only event must not be labeled true TTFT. Previous gateway-versus-bridge timings are small exploratory samples, not evidence of a causal bottleneck, capacity, overall stability or all accounts being healthy. Agent count is also not request concurrency: agents make multiple calls, pause for tools and carry different contexts.

## 8. Corrected implementation sequence and rollback

Stage A: freeze contracts and add deterministic failing regressions for the defects above. Preserve the current working configuration and establish rollback-compatible private-state backups without exposing or rewriting real credentials.

Stage B: repair model canonicalization, eligibility, cooldown parsing/preservation, auth refresh and stream/tool correctness. Unify bounded retry/error contracts before introducing new load. Keep default fallback off and requested-model fidelity intact.

Stage C: implement scheduler leases, configurable global/per-account capacity, bounded fair admission, cancellation/deadlines, backpressure and graceful drain. Keep a feature flag for routing policy, but do not roll back to a known stream corruption or authentication defect. Integrate all three API routes through the same admission policy.

Stage D: serialize durable state operations; implement real reload or document drain/restart. Add honest model/reasoning capability reporting and request telemetry. Validate OpenCode/Hermes payloads through fixtures; change client settings only with approval.

Stage E: with separate authorization, run a capped same-model live canary and compare the baseline against the scheduler on representative agent tasks. Tune ceilings/pacing from evidence. Do not raise the inventory cap simply to make a benchmark number bigger.

Stage F: reconcile existing Compose/Caddy/bootstrap/runbooks and test disposable Linux containers, then privately provision the actual VPS owner before public ingress. Deployment, DNS changes, account transfer and publication each need explicit permission. Verify remote HTTPS, owner login, key revocation, text, reasoning, tools, streaming/disconnect and restore/drain behavior.

Stage G: consider more accounts, adaptive weighting or replicas only if measured bottlenecks justify them. Multi-process design is a separate architecture change, not a constant edit.

Each stage requires its deterministic suite and previous-stage regression suite to pass. Operational rollout needs user-agreed latency/error/queue thresholds rather than invented SLO numbers. Roll back scheduler policy/configuration on sustained regression, leaked leases, unbounded queue growth, incorrect model use or protocol failures. Stop new admission, drain/cancel bounded work and retain compatible state; never restore stale credential/key files over active state casually. No rollback may reopen first-owner setup, re-enable revoked keys or turn off authentication.

## 9. Corrections, remaining unknowns and final recommendation

Withdraw the earlier unsupported figures: per-account 5-10 RPM, six-account 30-60 RPM, 3-6 agent capacity, 8+ agents waiting 5-15 seconds, fifteen-account 90-150 RPM, a 20-50 socket capacity claim, 100+ RPM after a cap change, 1.5-2.5 second requests and 2-3 times thinking-profile gains. None were established by concurrency/quota testing. Endpoint count does not multiply quota, round-robin does not prevent 429, sticky routing does not guarantee cache hits, and a few successful requests do not establish that every account is active.

UNKNOWN: actual account entitlements and policy compliance, shared quota scope across accounts/projects/models/endpoints, sustainable latency/throughput for the user's workload, provider behavior under the proposed reasoning controls, model transition compatibility, real client rendering of reasoning, and public VPS/TLS/recovery readiness. The source can prove mappings and algorithms, not these operational facts.

Recommended objective: maximize successful useful work under measured limits while bounding queueing, retries and failures. The minimum useful upgrade is correctness fixes -> account-aware bounded scheduler -> telemetry and controlled measurement. Default model downgrades, a thirty-account promise and additional load-balancing infrastructure are not prerequisites.

This is a completed review deliverable, not an implemented or production-qualified scaling upgrade. Gateway application/client code and services must remain unchanged by the review. Review verification is recorded separately in `.unlazy/scaling-plan-review/GATES.md`; it must not close the historical public-deployment requirement.
