import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomBytes } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { root, evidenceDir, newPassword } from './console-test-fixture.mjs';

const exec = promisify(execFile);
const suffix = randomBytes(8).toString('hex');
const name = 'agw-console-check-' + suffix;
const volume = name + '-data';
const image = 'antigravity-owner-console:local';
const origin = 'https://console-test.example.invalid';
const docker = async (...args) => (await exec('docker', args, { cwd: root, timeout: 180000, maxBuffer: 4 * 1024 * 1024 })).stdout.trim();
let containerCreated = false, volumeCreated = false;
let base;
async function healthy() {
  const deadline = Date.now() + 45000;
  while (Date.now() < deadline) {
    const state = JSON.parse(await docker('inspect', name))[0].State;
    if (!state.Running) throw new Error('Qualification container exited.');
    const current = JSON.parse(await docker('inspect', name))[0].NetworkSettings.Ports['8080/tcp']?.[0];
    if (current) base = `http://127.0.0.1:${current.HostPort}`;
    try {
      const response = await fetch(base + '/health', { signal: AbortSignal.timeout(1000) });
      if (response.status === 200) { assert.deepEqual(await response.json(), { status: 'ok' }); return; }
    } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error('Container readiness deadline exceeded.');
}
const send = (path, body, headers = {}, method = 'POST') => fetch(base + path, {
  method, headers: { Origin: origin, ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}), ...headers },
  ...(body !== undefined ? { body: JSON.stringify(body) } : {}), signal: AbortSignal.timeout(10000)
});
async function login(password) {
  const response = await send('/console/api/login', { username: 'container-test-owner', password });
  assert.equal(response.status, 200);
  const setCookie = response.headers.get('set-cookie');
  assert.match(setCookie, /^__Host-agw_session=/);
  assert.match(setCookie, /Secure/);
  assert.match(setCookie, /HttpOnly/);
  assert.match(setCookie, /SameSite=Strict/);
  return { Cookie: setCookie.split(';')[0], 'X-CSRF-Token': (await response.json()).csrf };
}
try {
  await docker('build', '-t', image, '.');
  const permissionCheck = await docker('run', '--rm', '--user', '0:0', '--read-only', '--tmpfs', '/tmp', '--mount', `type=bind,source=${join(root, 'tests/console/linux-permissions.mjs')},target=/checks/linux-permissions.mjs,readonly`, image, 'node', '/checks/linux-permissions.mjs');
  assert.ok(permissionCheck.includes('LINUX_PRIVATE_DATA_OK'));
  await docker('volume', 'create', '--label', 'purpose=agw-console-qualification', volume);
  volumeCreated = true;
  assert.equal((await docker('volume', 'inspect', volume)).includes(volume), true);
  // Use nocopy plus a root-owned mount to reproduce a fresh Linux bind source.
  await docker('run', '--rm', '--user', '0:0', '--mount', `type=volume,source=${volume},target=/app/data,volume-nocopy`, image, 'node', '-e', "const fs=require('node:fs');fs.chownSync('/app/data',0,0);fs.chmodSync('/app/data',493);");
  const initialized = await docker('run', '--rm', '--user', '0:0', '--network', 'none', '--read-only', '--cap-drop', 'ALL', '--cap-add', 'CHOWN', '--cap-add', 'FOWNER', '--cap-add', 'DAC_OVERRIDE', '--security-opt', 'no-new-privileges:true', '-v', volume + ':/app/data', image, 'node', 'scripts/prepare-data.mjs', '/app/data');
  assert.equal(initialized, 'PRIVATE_DATA_READY');
  await docker('run', '-d', '--name', name, '--label', 'purpose=agw-console-qualification', '--init', '--read-only', '--tmpfs', '/tmp', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true', '-p', '127.0.0.1::8080', '-e', 'PUBLIC_URL=' + origin, '-e', 'ACCOUNTS_FILE=/app/data/accounts.json', '-v', volume + ':/app/data', image);
  containerCreated = true;
  let inspected = JSON.parse(await docker('inspect', name))[0];
  assert.equal(inspected.Config.User, 'node');
  assert.equal(inspected.State.Running, true);
  const binding = inspected.NetworkSettings.Ports['8080/tcp'][0];
  assert.equal(binding.HostIp, '127.0.0.1');
  base = `http://127.0.0.1:${binding.HostPort}`;
  await healthy();
  const page = await fetch(base + '/console/');
  assert.equal(page.status, 200);
  assert.ok((await page.text()).includes('Create API key'));
  assert.equal((await fetch(base + '/console/app.js')).status, 200);
  assert.equal((await fetch(base + '/v1/models')).status, 401);
  const clean = await docker('exec', name, 'node', '-e', "const fs=require('node:fs'); if(fs.existsSync('/app/.env')||fs.existsSync('/app/data/accounts.json'))process.exit(1); console.log('NO_USER_CREDENTIALS_IN_IMAGE')");
  assert.equal(clean, 'NO_USER_CREDENTIALS_IN_IMAGE');
  assert.equal(await docker('exec', name, 'node', '-e', "if(require('node:fs').existsSync('/app/data/console/setup-token'))process.exit(1); console.log('NO_SETUP_TOKEN')"), 'NO_SETUP_TOKEN');
  const password = newPassword();
  let response = await send('/console/api/setup', { username: 'container-test-owner', password });
  assert.equal(response.status, 201);
  assert.equal((await send('/console/api/setup', { username: 'second-owner', password: newPassword() })).status, 409);
  let ownerHeaders = await login(password);
  response = await send('/console/api/keys', { name: 'Container qualification' }, ownerHeaders);
  assert.equal(response.status, 201);
  const { key, id } = await response.json();
  const keyHeaders = { Authorization: 'Bearer ' + key };
  assert.equal((await fetch(base + '/v1/key-check', { headers: keyHeaders })).status, 200);
  const mode = await docker('exec', name, 'node', '-e', "process.stdout.write(String(require('node:fs').statSync('/app/data/console/access.json').mode & 511))");
  assert.equal(Number(mode), 0o600);
  await docker('restart', name);
  await healthy();
  assert.equal((await send('/console/api/setup', { username: 'second-owner', password: newPassword() })).status, 409, 'restart must not reopen account creation');
  inspected = JSON.parse(await docker('inspect', name))[0];
  assert.equal(inspected.State.Running, true);
  assert.equal((await fetch(base + '/console/api/keys', { headers: ownerHeaders })).status, 401, 'restart must invalidate browser sessions');
  assert.equal((await fetch(base + '/v1/key-check', { headers: keyHeaders })).status, 200, 'key must survive restart');
  ownerHeaders = await login(password);
  response = await send('/console/api/keys/' + id, undefined, ownerHeaders, 'DELETE');
  assert.equal(response.status, 200);
  assert.equal((await fetch(base + '/v1/key-check', { headers: keyHeaders })).status, 401);
  await docker('restart', name);
  await healthy();
  assert.equal((await fetch(base + '/v1/key-check', { headers: keyHeaders })).status, 401, 'revocation must survive restart');
  mkdirSync(evidenceDir, { recursive: true });
  writeFileSync(join(evidenceDir, 'container.json'), JSON.stringify({ verifiedAt: new Date().toISOString(), imageId: inspected.Image, scope: 'local Docker; HTTPS-origin cookie behavior over isolated loopback transport, not a real public TLS certificate', checks: ['token-free-single-owner-setup', 'duplicate-setup-refused-after-restart', 'image-build', 'non-root', 'read-only-root', 'loopback-port', 'UI-and-assets', 'no-user-credentials-in-image', 'secure-cookie', 'private-file-permissions', 'hashed-key-persistence', 'session-invalidation-on-restart', 'revoke-persistence'] }, null, 2));
} finally {
  if (containerCreated) await docker('rm', '-f', name);
  if (volumeCreated) await docker('volume', 'rm', volume);
}
console.log('CONSOLE_CONTAINER_OK');
