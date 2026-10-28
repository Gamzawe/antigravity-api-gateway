// Executed only inside the isolated Linux qualification container; no real accounts.
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, statSync, rmSync, existsSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';

assert.equal(process.platform, 'linux');
const root = mkdtempSync(join(tmpdir(), 'agw-permissions-'));
const mode = path => statSync(path).mode & 0o777;
const account = { email: 'synthetic@example.invalid', source: 'oauth', refreshToken: 'SYNTHETIC_TEST_TOKEN_NOT_REAL' };
try {
  process.umask(0o022);
  const { saveAccounts } = await import('/app/src/account-manager/storage.js');
  const directory = join(root, 'new-private-state');
  const file = join(directory, 'accounts.json');
  await saveAccounts(file, [account], {}, 0);
  assert.equal(mode(directory), 0o700, 'new provider credential directory must be private');
  assert.equal(mode(file), 0o600, 'new provider credential file must be private');
  mkdirSync(join(root, 'existing'), { mode: 0o755 });
  const existing = join(root, 'existing', 'accounts.json');
  writeFileSync(existing, '{}', { mode: 0o644 });
  await saveAccounts(existing, [account], {}, 0);
  assert.equal(mode(join(root, 'existing')), 0o700);
  assert.equal(mode(existing), 0o600, 'existing loose credential permissions must be repaired');
  assert.equal(JSON.parse(readFileSync(existing, 'utf8')).accounts[0].email, account.email);
  // A permission failure must occur before the owner commit, never reopen a committed owner.
  const { AccessStore } = await import('/app/src/console/access.mjs');
  const ownerDir = join(root, 'owner-commit-check');
  const store = new AccessStore(ownerDir);
  const credentials = { username: 'permission-test-owner', password: 'synthetic-permission-test-password' };
  const chmod = fs.chmodSync;
  try {
    fs.chmodSync = () => { throw new Error('Synthetic chmod failure'); };
    syncBuiltinESMExports();
    await assert.rejects(store.setup(credentials), /Synthetic chmod failure/);
    assert.equal(store.isConfigured(), false);
    assert.equal(existsSync(join(ownerDir, 'access.json')), false, 'permission failure must not commit an owner and reopen setup');
  } finally { fs.chmodSync = chmod; syncBuiltinESMExports(); }
  await store.setup(credentials);
  assert.equal(mode(join(ownerDir, 'access.json')), 0o600);
  await assert.rejects(store.setup({ username: 'replacement', password: credentials.password }), error => error.status === 409);
  assert.equal((await new AccessStore(ownerDir).login(credentials.username, credentials.password)).username, credentials.username);
  // Reproduce the bind-source ownership state caused by a root SSH/scp upload.
  const mount = join(root, 'root-owned-data');
  mkdirSync(mount, { mode: 0o755 });
  writeFileSync(join(mount, 'accounts.json'), JSON.stringify({ accounts: [] }), { mode: 0o644 });
  mkdirSync(join(mount, 'console'), { mode: 0o755 });
  writeFileSync(join(mount, 'console', 'access.json'), '{"synthetic":true}', { mode: 0o644 });
  assert.equal(statSync(mount).uid, 0);
  const helper = '/app/scripts/prepare-data.mjs';
  assert.equal(existsSync(helper), true, 'data permission initializer must exist');
  let child = spawnSync(process.execPath, [helper, mount], { encoding: 'utf8' });
  assert.equal(child.status, 0, child.stderr);
  for (const path of [mount, join(mount, 'console')]) {
    assert.equal(statSync(path).uid, 1000); assert.equal(mode(path), 0o700);
  }
  for (const path of [join(mount, 'accounts.json'), join(mount, 'console', 'access.json')]) {
    assert.equal(statSync(path).uid, 1000); assert.equal(mode(path), 0o600);
  }
  child = spawnSync(process.execPath, ['-e', "const fs=require('node:fs');fs.mkdirSync(process.argv[1]+'/write-check');fs.writeFileSync(process.argv[1]+'/write-check/ok','OK');", mount], { uid: 1000, gid: 1000, encoding: 'utf8' });
  // Parent fixture is mode 700, so allow traversal ONLY for this synthetic subtree.
  if (child.status !== 0) {
    const { chmodSync } = await import('node:fs'); chmodSync(root, 0o711);
    child = spawnSync(process.execPath, ['-e', "const fs=require('node:fs');fs.mkdirSync(process.argv[1]+'/write-check');fs.writeFileSync(process.argv[1]+'/write-check/ok','OK');", mount], { uid: 1000, gid: 1000, encoding: 'utf8' });
  }
  assert.equal(child.status, 0, 'initialized data must be writable by container UID 1000');
  assert.equal(readFileSync(join(mount, 'write-check', 'ok'), 'utf8'), 'OK');
  const outside = join(root, 'must-not-change'); mkdirSync(outside, { mode: 0o755 });
  symlinkSync(outside, join(mount, 'outside-link'));
  child = spawnSync(process.execPath, [helper, mount], { encoding: 'utf8' });
  assert.notEqual(child.status, 0, 'initializer must reject symlinks instead of touching an outside path');
  assert.equal(statSync(outside).uid, 0); assert.equal(mode(outside), 0o755);
} finally { rmSync(root, { recursive: true, force: true }); }
console.log('LINUX_PRIVATE_DATA_OK');
