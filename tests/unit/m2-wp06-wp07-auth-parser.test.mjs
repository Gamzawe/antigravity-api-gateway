import test from 'node:test';
import assert from 'node:assert/strict';
import { getTokenForAccount } from '../../src/account-manager/credentials.js';
import { parseResetTime } from '../../src/cloudcode/rate-limit-parser.js';

test('WP-06: concurrent calls to getTokenForAccount trigger single in-flight OAuth refresh', async () => {
    let refreshCalls = 0;
    const tokenCache = new Map();
    const fakeAccount = {
        email: 'singleflight@example.com',
        source: 'oauth',
        refreshToken: 'valid-refresh-token'
    };

    // Override global fetch inside refreshAccessToken
    const originalFetch = global.fetch;
    try {
        global.fetch = async () => {
            refreshCalls++;
            // Simulate 50ms network delay
            await new Promise(r => setTimeout(r, 50));
            return new Response(JSON.stringify({
                access_token: 'new-refreshed-token',
                expires_in: 3600
            }), { status: 200, headers: { 'content-type': 'application/json' } });
        };

        // Fire 5 concurrent requests simultaneously
        const results = await Promise.all([
            getTokenForAccount(fakeAccount, tokenCache),
            getTokenForAccount(fakeAccount, tokenCache),
            getTokenForAccount(fakeAccount, tokenCache),
            getTokenForAccount(fakeAccount, tokenCache),
            getTokenForAccount(fakeAccount, tokenCache)
        ]);

        assert.equal(refreshCalls, 1, 'Must execute exactly one token refresh call for concurrent requests');
        assert.ok(results.every(t => t === 'new-refreshed-token'), 'All concurrent callers must receive fresh token');
    } finally {
        global.fetch = originalFetch;
    }
});

test('WP-07: parseResetTime parses decimal seconds and milliseconds accurately', () => {
    // Test decimal seconds: 1.5s -> 1500ms
    const msg1 = 'Resource exhausted. quotaResetDelay: "1.5s". Please retry.';
    const reset1 = parseResetTime(null, msg1);
    assert.equal(reset1, 1500, '1.5s must parse to 1500ms');

    // Test decimal milliseconds: 754.43ms -> under 1000ms is clamped to safe 2000ms buffer by rate-limit-parser
    const msg2 = 'quotaResetDelay: "754.43ms"';
    const reset2 = parseResetTime(null, msg2);
    assert.equal(reset2, 2000, 'Sub-second reset delay must enforce minimum 2000ms safety buffer');

    // Test retryDelay in seconds: 5.2s -> 5200ms
    const msg3 = 'retryDelay: "5.2s"';
    const reset3 = parseResetTime(null, msg3);
    assert.equal(reset3, 5200, '5.2s must parse to 5200ms');
});
