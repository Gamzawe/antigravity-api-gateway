import test from 'node:test';
import assert from 'node:assert/strict';
import { AccountManager } from '../../src/account-manager/index.js';
import { LeaseEngine } from '../../src/account-manager/lease-engine.js';
import { AdmissionQueue } from '../../src/scheduler/admission-queue.js';

test('Balancing: LeaseEngine selects highest remaining quota fraction when in-flight is equal', () => {
    const engine = new LeaseEngine({ maxPerAccount: 2, maxTotal: 10 });
    const accounts = [
        {
            email: 'lower-quota@gmail.com',
            enabled: true,
            modelQuotas: {
                'gemini-3.8-flash-tiered': { remainingFraction: 0.85 }
            }
        },
        {
            email: 'higher-quota@gmail.com',
            enabled: true,
            modelQuotas: {
                'gemini-3.8-flash-tiered': { remainingFraction: 0.99 }
            }
        }
    ];

    // Both have 0 in-flight streams. Higher fraction should be chosen.
    const res = engine.acquireLease(accounts, 'gemini-3.8-flash');
    assert.ok(res);
    assert.equal(res.account.email, 'higher-quota@gmail.com', 'Should balance towards account with healthier quota');
    res.lease.release();
});

test('Balancing: AccountManager manages atomic in-flight counts and lease releases', async () => {
    const am = new AccountManager('tests/fixtures/dummy-empty.json', { maxPerAccount: 2, maxTotal: 5 });
    // Initialize loads default/configured accounts
    await am.initialize();

    const l1 = am.acquireLease('gemini-3.8-flash');
    const l2 = am.acquireLease('gemini-3.8-flash');

    assert.ok(l1);
    assert.ok(l2);
    // Verified 2 distinct leases
    assert.equal(am.getTotalInFlight(), 2);

    l1.lease.release();
    assert.equal(am.getTotalInFlight(), 1);

    l2.lease.release();
    assert.equal(am.getTotalInFlight(), 0);
});

test('AdmissionQueue: execute dispatched successfully', async () => {
    const q = new AdmissionQueue({ maxDepth: 10, maxBytes: 50000 });
    let executed = false;

    const p = q.enqueue({
        key: 'test',
        payloadBytes: 100,
        execute: async () => {
            executed = true;
            return 'OK';
        }
    });

    await q.processNext();
    const res = await p;

    assert.equal(executed, true);
    assert.equal(res, 'OK');
});
