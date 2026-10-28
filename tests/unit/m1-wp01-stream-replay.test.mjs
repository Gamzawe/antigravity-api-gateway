import test from 'node:test';
import assert from 'node:assert/strict';
import { sendMessageStream } from '../../src/cloudcode/streaming-handler.js';

test('WP-01: stream fails terminally without retrying once downstream yield has occurred', async () => {
    let callCount = 0;
    const fakeAccount = { email: 'test@example.com', name: 'Test' };
    const fakeAccountManager = {
        pickStickyAccount: () => ({ account: fakeAccount, waitMs: 0 }),
        getCurrentStickyAccount: () => fakeAccount,
        isAllRateLimited: () => false,
        getAccountCount: () => 2,
        getTokenForAccount: async () => 'fake-token',
        getProjectForAccount: async () => 'fake-project',
        clearExpiredLimits: () => {},
        markRateLimited: () => {},
        pickNext: () => fakeAccount
    };

    // Override global fetch
    const originalFetch = global.fetch;
    try {
        global.fetch = async (url) => {
            callCount++;
            if (callCount === 1) {
                const parts = [
                    'data: {"response":{"candidates":[{"content":{"parts":[{"text":"Hello world"}]}}]}}\n\n',
                ];
                let count = 0;
                const stream = new ReadableStream({
                    async pull(controller) {
                        if (count < parts.length) {
                            controller.enqueue(new TextEncoder().encode(parts[count++]));
                        } else {
                            // Drop network immediately after emitting
                            controller.error(new Error('Network connection abruptly dropped'));
                        }
                    }
                });
                return new Response(stream, {
                    status: 200,
                    headers: { 'content-type': 'text/event-stream' }
                });
            }
            // If it retried, it would call a second time
            const stream = new ReadableStream({
                start(controller) {
                    const encoder = new TextEncoder();
                    controller.enqueue(encoder.encode('data: {"candidates":[{"content":{"parts":[{"text":"Should not be reached"}]}}]}\n\n'));
                    controller.close();
                }
            });
            return new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } });
        };

        const yielded = [];
        let caughtError = null;
        try {
            for await (const event of sendMessageStream({ model: 'gemini-3.8-flash', messages: [{ role: 'user', content: 'test' }] }, fakeAccountManager, false)) {
                yielded.push(event);
            }
        } catch (err) {
            caughtError = err;
        }

        assert.ok(yielded.length > 0, 'Should have yielded partial event before network drop');
        assert.ok(caughtError, 'Should have thrown error rather than swallowed or retried');
        assert.equal(callCount, 1, 'Should NOT have retried upstream after partial downstream yield');
    } finally {
        global.fetch = originalFetch;
    }
});
