import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { AccessStore } from '../../src/console/access.mjs';

test('created API keys reveal once, survive restart hashed, revoke immediately and stay revoked', async () => {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-keys-'));
  try {
    const store = new AccessStore(dir);
    assert.equal(typeof store.createKey, 'function', 'key lifecycle is missing');
    const created = store.createKey('Phone');
    assert.match(created.key, /^sk-agw-[A-Za-z0-9_-]{43}$/);
    assert.ok(!readFileSync(join(dir, 'access.json'), 'utf8').includes(created.key));
    const metadata = store.listKeys();
    assert.equal(metadata.length, 1);
    assert.equal(metadata[0].name, 'Phone');
    assert.equal(metadata[0].key, undefined);
    assert.equal(metadata[0].hash, undefined);
    assert.ok(!JSON.stringify(metadata).includes(created.key));
    assert.equal(store.verifyKey('not-valid'), null);
    const restarted = new AccessStore(dir);
    assert.equal(restarted.verifyKey(created.key).id, created.id);
    assert.ok(restarted.listKeys()[0].lastUsedAt);
    assert.throws(() => restarted.createKey(''), /name/i);
    assert.throws(() => restarted.createKey('x'.repeat(81)), /name/i);
    assert.equal(restarted.revokeKey(created.id).revokedAt !== null, true);
    assert.equal(restarted.verifyKey(created.key), null);
    assert.equal(new AccessStore(dir).verifyKey(created.key), null);
    assert.throws(() => restarted.revokeKey('missing'), /not found/i);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('owner sessions expire on time and do not act as API keys', async () => {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-expiry-'));
  try {
    let now = 1000;
    const store = new AccessStore(dir, { now: () => now, sessionMs: 100 });
    await store.setup({ username: 'me', password: 'a-strong-test-password' });
    const session = await store.login('me', 'a-strong-test-password');
    assert.equal(typeof store.verifyKey, 'function', 'separate API-key auth is missing');
    assert.equal(store.verifyKey(session.token), null);
    now = 1100;
    assert.equal(store.session(session.token), null);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
