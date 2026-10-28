#!/usr/bin/env node

/**
 * Export Local Antigravity Accounts
 *
 * Reads active Antigravity accounts from local OpenCode or Antigravity config
 * and exports them to ./data/accounts.json ready for VPS deployment.
 */

import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs';
import { join, resolve } from 'path';
import { homedir } from 'os';

const candidatePaths = [
    join(homedir(), '.config', 'opencode', 'antigravity-accounts.json'),
    join(homedir(), '.config', 'antigravity-gateway', 'accounts.json'),
    resolve(process.cwd(), 'accounts.json')
];

let sourcePath = process.argv[2];
if (!sourcePath) {
    sourcePath = candidatePaths.find(p => existsSync(p));
}

if (!sourcePath || !existsSync(sourcePath)) {
    console.error('❌ Could not locate any local antigravity accounts file.');
    console.error('Searched paths:');
    candidatePaths.forEach(p => console.error(' - ' + p));
    process.exit(1);
}

console.log(`🔍 Reading accounts from: ${sourcePath}`);
const raw = JSON.parse(readFileSync(sourcePath, 'utf8'));

const accountsList = Array.isArray(raw.accounts) ? raw.accounts : (Array.isArray(raw) ? raw : []);
const exportableAccounts = accountsList
    .filter(acc => acc.refreshToken && acc.enabled !== false)
    .map(acc => ({
        email: acc.email,
        refreshToken: acc.refreshToken,
        source: 'oauth',
        projectId: acc.managedProjectId || acc.projectId || 'aicode-consumers',
        managedProjectId: acc.managedProjectId || acc.projectId || 'aicode-consumers',
        enabled: true,
        label: acc.label || acc.email,
        fingerprint: acc.fingerprint || undefined,
        addedAt: acc.addedAt || Date.now()
    }));

if (exportableAccounts.length === 0) {
    console.error('❌ No active accounts with refresh tokens found in source file.');
    process.exit(1);
}

const targetDir = resolve(process.cwd(), 'data');
if (!existsSync(targetDir)) {
    mkdirSync(targetDir, { recursive: true });
}

const targetFile = join(targetDir, 'accounts.json');
const payload = {
    version: 1,
    accounts: exportableAccounts,
    settings: {
        cooldownMs: 60000,
        stickySession: true
    },
    exportedAt: new Date().toISOString()
};

writeFileSync(targetFile, JSON.stringify(payload, null, 2), 'utf8');

console.log(`\n✅ Successfully exported ${exportableAccounts.length} account(s) to:`);
console.log(`   ${targetFile}`);
console.log('\nExported accounts:');
exportableAccounts.forEach((acc, i) => {
    console.log(` ${i + 1}. ${acc.email} (Project: ${acc.projectId})`);
});
console.log('\n👉 You can now copy the ./data folder or accounts.json directly to your VPS!');
