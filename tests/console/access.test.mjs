import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, statSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

test('one owner creates an account without a setup token; password and session secrets are never stored raw', async () => {
  const module = await import('../../src/console/access.mjs').catch(() => null);
  assert.ok(module?.AccessStore, 'AccessStore must implement private owner setup');
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-access-'));
  try {
    const store = new module.AccessStore(dir);
    assert.equal(existsSync(join(dir, 'setup-token')), false);
    await assert.rejects(store.setup({ username: 'owner', password: 'short' }), /12/);
    await store.setup({ username: 'owner', password: 'a-strong-test-password' });
    assert.equal(store.isConfigured(), true);
    await assert.rejects(store.setup({ username: 'intruder', password: 'another-long-password' }), /already/i);
    await assert.rejects(store.login('owner', 'bad-password'), /invalid/i);
    const session = await store.login('owner', 'a-strong-test-password');
    assert.ok(session.token.length >= 40);
    assert.equal(store.session(session.token).username, 'owner');
    assert.ok(!readFileSync(join(dir, 'access.json'), 'utf8').includes('a-strong-test-password'));
    assert.ok(!readFileSync(join(dir, 'access.json'), 'utf8').includes(session.token));
    store.logout(session.token);
    assert.equal(store.session(session.token), null);
    const restarted = new module.AccessStore(dir);
    assert.equal(restarted.isConfigured(), true);
    assert.equal(restarted.session(session.token), null);
    assert.ok((await restarted.login('owner', 'a-strong-test-password')).token);
    if (process.platform !== 'win32') assert.equal(statSync(join(dir, 'access.json')).mode & 0o777, 0o600);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
