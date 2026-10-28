import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { AccessStore } from '../../src/console/access.mjs';

test('failed revocation persistence is never acknowledged on a retry and preserves a consistent state', () => {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-revoke-fail-'));
  try {
    const store = new AccessStore(dir);
    const { key, id } = store.createKey('App');
    const originalSave = store.save.bind(store);
    store.save = () => { throw new Error('Simulated disk failure'); };
    assert.throws(() => store.revokeKey(id), /disk failure/);
    assert.throws(() => store.revokeKey(id), /disk failure/, 'a second attempt must not silently succeed without persisted revocation');
    store.save = originalSave;
    store.revokeKey(id);
    assert.equal(new AccessStore(dir).verifyKey(key), null);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('a failed last-used write remains a failed authentication until persistence recovers', () => {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-last-used-fail-'));
  try {
    const store = new AccessStore(dir);
    const { key, id } = store.createKey('App');
    const originalSave = store.save.bind(store);
    store.save = () => { throw new Error('Simulated disk failure'); };
    assert.throws(() => store.verifyKey(key), /disk failure/);
    assert.throws(() => store.verifyKey(key), /disk failure/);
    store.save = originalSave;
    assert.equal(store.verifyKey(key).id, id);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
