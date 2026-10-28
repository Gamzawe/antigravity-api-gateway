import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash, scryptSync } from 'node:crypto';
import { AccessStore } from '../../src/console/access.mjs';

const temporary = () => mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-single-owner-'));

test('upgrading preserves the legacy owner, hashed key and revocation state', async () => {
  const dir = temporary();
  try {
    const username = 'legacy-owner', password = 'legacy-owner-test-password', salt = 'synthetic-legacy-salt';
    const hash = scryptSync(password, salt, 64, { N: 32768, r: 8, p: 1, maxmem: 64 * 1024 * 1024 }).toString('hex');
    const rawKey = 'sk-agw-' + 'A'.repeat(43);
    const legacy = { version: 1, owner: { username, salt, hash, createdAt: '2026-01-01T00:00:00.000Z' }, keys: [{ id: 'legacy-key', name: 'Existing app', prefix: rawKey.slice(0, 14), hash: createHash('sha256').update(rawKey).digest('hex'), createdAt: '2026-01-01T00:00:00.000Z', lastUsedAt: '2026-01-01T00:00:00.000Z', revokedAt: null }] };
    const file = join(dir, 'access.json');
    writeFileSync(file, JSON.stringify(legacy));
    writeFileSync(join(dir, 'setup-token'), 'OBSOLETE_TEST_TOKEN');
    const before = readFileSync(file, 'utf8');
    const store = new AccessStore(dir, { now: () => Date.parse('2026-01-01T00:00:00.000Z') });
    assert.equal((await store.login(username, password)).username, username);
    await assert.rejects(store.setup({ username: 'replacement', password }), error => error.status === 409);
    assert.equal(store.verifyKey(rawKey).id, 'legacy-key');
    assert.equal(readFileSync(file, 'utf8'), before, 'loading and logging in must not reset existing state');
    store.revokeKey('legacy-key');
    assert.equal(new AccessStore(dir).verifyKey(rawKey), null);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('failed first-owner persistence does not acknowledge success and permits safe retry', async () => {
  const dir = temporary();
  try {
    const store = new AccessStore(dir);
    const save = store.save.bind(store);
    store.save = () => { throw new Error('Synthetic disk failure'); };
    const credentials = { username: 'owner', password: 'single-owner-test-password' };
    await assert.rejects(store.setup(credentials), /Synthetic disk failure/);
    assert.equal(store.isConfigured(), false);
    assert.equal(new AccessStore(dir).isConfigured(), false);
    store.save = save;
    await store.setup(credentials);
    assert.equal((await new AccessStore(dir).login(credentials.username, credentials.password)).username, credentials.username);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('invalid username and password cannot claim the single account', async () => {
  const dir = temporary();
  try {
    const store = new AccessStore(dir);
    await assert.rejects(store.setup({ username: '', password: 'single-owner-test-password' }), error => error.status === 400);
    await assert.rejects(store.setup({ username: 'owner', password: 'short' }), error => error.status === 400);
    assert.equal(store.isConfigured(), false);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('an obsolete setup-token file is ignored and never required', async () => {
  const dir = temporary();
  try {
    writeFileSync(join(dir, 'setup-token'), 'OBSOLETE_TEST_TOKEN');
    const store = new AccessStore(dir);
    await store.setup({ username: 'owner', password: 'single-owner-test-password' });
    assert.equal(readFileSync(join(dir, 'setup-token'), 'utf8'), 'OBSOLETE_TEST_TOKEN');
    assert.equal(store.isConfigured(), true);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('a fresh store does not generate or consume a setup-token file', async () => {
  const dir = temporary();
  try {
    const store = new AccessStore(dir);
    assert.equal(existsSync(join(dir, 'setup-token')), false, 'fresh stores must not generate setup tokens');
    await store.setup({ username: 'owner', password: 'single-owner-test-password' });
    assert.equal(existsSync(join(dir, 'setup-token')), false);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('username/password-only setup admits exactly one owner even with concurrent attempts and restart', async () => {
  const dir = temporary();
  try {
    const store = new AccessStore(dir);
    const candidates = [
      { username: 'first-owner', password: 'first-owner-test-password' },
      { username: 'second-owner', password: 'second-owner-test-password' }
    ];
    const outcomes = await Promise.allSettled(candidates.map(credentials => store.setup(credentials)));
    assert.equal(outcomes.filter(result => result.status === 'fulfilled').length, 1, 'one owner must be created without a setup token');
    assert.equal(outcomes.filter(result => result.status === 'rejected' && result.reason.status === 409).length, 1);
    const winner = candidates[outcomes.findIndex(result => result.status === 'fulfilled')];
    const loser = candidates[outcomes.findIndex(result => result.status === 'rejected')];
    const restarted = new AccessStore(dir);
    await assert.rejects(restarted.setup(loser), error => error.status === 409);
    assert.equal((await restarted.login(winner.username, winner.password)).username, winner.username);
    await assert.rejects(restarted.login(loser.username, loser.password), error => error.status === 401);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
