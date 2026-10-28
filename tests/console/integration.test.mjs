import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-integration-'));
process.env.DATA_DIR = dir;
process.env.ACCOUNTS_FILE = join(dir, 'empty-accounts.json');
process.env.PUBLIC_URL = 'http://localhost:8089';
process.env.API_KEYS = '';
process.env.API_KEY = '';
process.env.FALLBACK = 'false';

test('generated keys integrate with the real server; no-key configuration is closed, management isolated and local files rejected', async () => {
  const { default: app } = await import('../../src/server.js');
  const server = app.listen(0, '127.0.0.1');
  await new Promise(resolve => server.once('listening', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const origin = process.env.PUBLIC_URL;
  const request = (path, body, extra = {}) => fetch(base + path, { method: 'POST', headers: { Origin: origin, 'Content-Type': 'application/json', ...extra }, body: JSON.stringify(body) });
  try {
    const unauth = await fetch(base + '/v1/models');
    assert.equal(unauth.status, 401, 'no configured keys must not allow anonymous inference');
    const health = await (await fetch(base + '/health')).json();
    assert.deepEqual(Object.keys(health).sort(), ['status'], 'public health must not expose accounts or quotas');
    assert.equal((await fetch(base + '/')).status, 200);
    const setup = await request('/console/api/setup', { username: 'owner', password: 'my-private-test-password' });
    assert.equal(setup.status, 201);
    let r = await request('/console/api/login', { username: 'owner', password: 'my-private-test-password' });
    assert.equal(r.status, 200);
    const cookie = r.headers.get('set-cookie').split(';')[0];
    const session = await r.json();
    r = await request('/console/api/keys', { name: 'test app' }, { Cookie: cookie, 'X-CSRF-Token': session.csrf });
    assert.equal(r.status, 201);
    const { key, id } = await r.json();
    const headers = { Authorization: 'Bearer ' + key };
    const who = await (await fetch(base + '/v1/key-check', { headers })).json();
    assert.equal(who.keyId, id);
    assert.equal((await fetch(base + '/v1/key-check', { headers: { 'x-api-key': key } })).status, 200);
    assert.equal((await fetch(base + '/v1/key-check', { headers: { Cookie: cookie } })).status, 401);
    for (const route of ['/v1/accounts/status', '/account-limits']) assert.equal((await fetch(base + route, { headers })).status, 401);
    assert.equal((await request('/v1/accounts/reload', {}, headers)).status, 401);
    const payload = { model: 'gemini-3.8-flash', messages: [{ role: 'user', content: [{ type: 'file', url: 'file:///etc/passwd' }] }] };
    r = await request('/v1/chat/completions', payload, headers);
    assert.equal(r.status, 400);
    assert.match((await r.json()).error.message, /local file/i);
    for (const path of ['/v1/messages', '/v1/responses']) {
      assert.equal((await request(path, { model: 'anything', messages: [{ content: [{ type: 'document', source: { type: 'url', url: 'file:///C:/secrets.txt' } }] }] }, headers)).status, 400);
    }
    const localPath = { type: 'file', url: '/etc/passwd' };
    assert.equal((await request('/v1/chat/completions', { model: 'anything', messages: [{ role: 'user', content: [localPath] }] }, headers)).status, 400);
    assert.equal((await request('/v1/responses', { model: 'anything', input: [{ role: 'user', content: [{ type: 'input_file', file_url: '/etc/passwd' }] }] }, headers)).status, 400);
    // Ordinary prose about a URI must not be mistaken for an attachment.
    r = await request('/unsupported', { messages: [{ role: 'user', content: 'Explain the file: URI scheme.' }] }, headers);
    assert.equal(r.status, 404);
    r = await fetch(base + '/console/api/keys/' + id, { method: 'DELETE', headers: { Origin: origin, Cookie: cookie, 'X-CSRF-Token': session.csrf } });
    assert.equal(r.status, 200);
    assert.equal((await fetch(base + '/v1/key-check', { headers })).status, 401);
    process.env.API_KEYS = 'legacy-private-test';
    assert.equal((await fetch(base + '/v1/key-check', { headers: { Authorization: 'Bearer legacy-private-test' } })).status, 200);
    assert.equal((await fetch(base + '/account-limits', { headers: { Authorization: 'Bearer legacy-private-test' } })).status, 401);
    assert.ok(!readFileSync(join(dir, 'console', 'access.json'), 'utf8').includes(key));
  } finally { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); rmSync(dir, { recursive: true, force: true }); }
});
