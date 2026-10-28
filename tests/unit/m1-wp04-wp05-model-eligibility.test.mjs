import test from 'node:test';
import assert from 'node:assert/strict';
import { isAllRateLimited, getAvailableAccounts, markRateLimited } from '../../src/account-manager/rate-limits.js';
import { isAccountUsable } from '../../src/account-manager/selection.js';

test('WP-04 & WP-05: rate limit registered under alias or canonical ID scopes to canonical model', () => {
    const accounts = [
        { email: 'acc1@gmail.com', enabled: true, isInvalid: false, modelRateLimits: {} },
        { email: 'acc2@gmail.com', enabled: true, isInvalid: false, modelRateLimits: {} }
    ];

    // Mark acc1 rate-limited using alias 'gemini-3.8-flash'
    markRateLimited(accounts, 'acc1@gmail.com', 60000, {}, 'gemini-3.8-flash');

    // Both alias and canonical checks must reflect rate limit
    const availableUnderAlias = getAvailableAccounts(accounts, 'gemini-3.8-flash');
    const availableUnderCanonical = getAvailableAccounts(accounts, 'gemini-3.8-flash-tiered');

    assert.equal(availableUnderAlias.length, 1, 'Only acc2 should be available under alias query');
    assert.equal(availableUnderCanonical.length, 1, 'Only acc2 should be available under canonical query');
    assert.equal(availableUnderAlias[0].email, 'acc2@gmail.com');
    assert.equal(availableUnderCanonical[0].email, 'acc2@gmail.com');
});

test('WP-05: accounts marked enabled: false are excluded from selection and availability', () => {
    const accounts = [
        { email: 'disabled@gmail.com', enabled: false, isInvalid: false },
        { email: 'active@gmail.com', enabled: true, isInvalid: false }
    ];

    const available = getAvailableAccounts(accounts, 'gemini-3.8-flash');
    assert.equal(available.length, 1);
    assert.equal(available[0].email, 'active@gmail.com');

    assert.equal(isAccountUsable(accounts[0], 'gemini-3.8-flash'), false, 'Disabled account must return false for isAccountUsable');
    assert.equal(isAccountUsable(accounts[1], 'gemini-3.8-flash'), true, 'Active account must return true for isAccountUsable');
});
