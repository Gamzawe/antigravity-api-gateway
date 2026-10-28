import test from 'node:test';
import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { unlinkSync, existsSync } from 'node:fs';
import { saveAccounts, loadAccounts } from '../../src/account-manager/storage.js';

test('WP-08: rapid concurrent saveAccounts calls execute sequentially without file corruption', async () => {
    const testConfigPath = resolve('D:/Side-Projects/antigravity-api-gateway/.unlazy/test-concurrent-accounts.json');
    if (existsSync(testConfigPath)) unlinkSync(testConfigPath);

    try {
        const baseAccounts = [
            { email: 'user1@example.com', source: 'oauth', refreshToken: 'tok1' }
        ];

        // Launch 10 rapid concurrent saves with incrementing activeIndex
        const saves = [];
        for (let i = 0; i < 10; i++) {
            saves.push(saveAccounts(testConfigPath, baseAccounts, {}, i));
        }

        await Promise.all(saves);

        // Load back and verify file is valid JSON and not corrupt
        const loaded = await loadAccounts(testConfigPath);
        assert.equal(loaded.accounts.length, 1);
        assert.equal(loaded.accounts[0].email, 'user1@example.com');
        assert.ok(typeof loaded.activeIndex === 'number');
    } finally {
        if (existsSync(testConfigPath)) unlinkSync(testConfigPath);
    }
});
