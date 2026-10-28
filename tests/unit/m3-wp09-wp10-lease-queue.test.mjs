import test from 'node:test';
import assert from 'node:assert/strict';
import { LeaseEngine } from '../../src/account-manager/lease-engine.js';
import { AdmissionQueue } from '../../src/scheduler/admission-queue.js';

test('WP-09: LeaseEngine allocates attempt lease atomically and releases idempotently', () => {
    const engine = new LeaseEngine({ maxPerAccount: 1, maxTotal: 5 });
    const accounts = [
        { email: 'acc1@gmail.com', enabled: true },
        { email: 'acc2@gmail.com', enabled: true }
    ];

    // First lease should pick acc1
    const res1 = engine.acquireLease(accounts, 'gemini-3.8-flash');
    assert.ok(res1);
    assert.equal(res1.account.email, 'acc1@gmail.com');
    assert.equal(engine.getInFlight('acc1@gmail.com'), 1);

    // Second lease must balance to acc2
    const res2 = engine.acquireLease(accounts, 'gemini-3.8-flash');
    assert.ok(res2);
    assert.equal(res2.account.email, 'acc2@gmail.com');
    assert.equal(engine.getInFlight('acc2@gmail.com'), 1);

    // Third lease should fail because both are at maxPerAccount (1)
    const res3 = engine.acquireLease(accounts, 'gemini-3.8-flash');
    assert.equal(res3, null, 'Should return null when capacity is saturated');

    // Release res1 idempotently
    res1.lease.release();
    assert.equal(engine.getInFlight('acc1@gmail.com'), 0);
    res1.lease.release(); // duplicate release
    assert.equal(engine.getInFlight('acc1@gmail.com'), 0);

    // Now acc1 is available again
    const res4 = engine.acquireLease(accounts, 'gemini-3.8-flash');
    assert.ok(res4);
    assert.equal(res4.account.email, 'acc1@gmail.com');
});

test('WP-10: AdmissionQueue enforces depth limit and prunes aborted requests', async () => {
    const queue = new AdmissionQueue({ maxDepth: 2, maxBytes: 10000 });

    const controller = new AbortController();
    const p1 = queue.enqueue({ key: 'key1', payloadBytes: 100 });
    const p2 = queue.enqueue({ key: 'key2', payloadBytes: 100, abortSignal: controller.signal });

    assert.equal(queue.depth, 2);

    // Enqueue 3rd should reject with QUEUE_FULL
    await assert.rejects(
        queue.enqueue({ key: 'key3', payloadBytes: 100 }),
        /QUEUE_FULL/
    );

    // Abort p2
    controller.abort();
    await assert.rejects(p2, /REQUEST_ABORTED/);

    // Queue depth should automatically decrement to 1
    assert.equal(queue.depth, 1);
});
