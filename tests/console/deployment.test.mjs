import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '../..');
function start(cwd, env) {
  const child = spawn(process.execPath, [join(root, 'src/index.js')], { cwd, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
  let output = ''; child.stdout.on('data', data => { output += data; }); child.stderr.on('data', data => { output += data; });
  return { child, output: () => output };
}
async function waitUntil(fn, budget = 5000) { const until = Date.now() + budget; while (Date.now() < until) { if (await fn()) return; await new Promise(r => setTimeout(r, 30)); } throw new Error('Startup deadline exceeded.'); }

test('production refuses HTTP origin; .env is loaded before constants and server is loopback-only by default', async () => {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-startup-'));
  let child;
  try {
    let result = start(dir, { NODE_ENV: 'production', PUBLIC_URL: 'http://unsafe.test', PORT: '0', DATA_DIR: dir, API_KEYS: '' });
    child = result.child;
    await waitUntil(() => child.exitCode !== null);
    assert.notEqual(child.exitCode, 0);
    assert.match(result.output(), /HTTPS_REQUIRED/);
    writeFileSync(join(dir, '.env'), 'HOST=127.0.0.1\nPORT=0\nPUBLIC_URL=http://localhost:8089\nDATA_DIR=./private-state\nAPI_KEYS=\n');
    result = start(dir, { NODE_ENV: 'test' });
    child = result.child;
    await waitUntil(() => /"ready":true/.test(result.output()) || child.exitCode !== null);
    assert.equal(child.exitCode, null, result.output());
    const ready = result.output().split('\n').filter(Boolean).map(line => { try { return JSON.parse(line); } catch { return null; } }).find(item => item?.ready);
    assert.equal(ready.host, '127.0.0.1');
    assert.ok(ready.port > 0);
    assert.ok(existsSync(join(dir, 'private-state', 'console')), 'DATA_DIR from .env must take effect before imports');
    assert.equal(existsSync(join(dir, 'private-state', 'console', 'setup-token')), false);
    const health = await fetch(`http://127.0.0.1:${ready.port}/health`);
    assert.deepEqual(await health.json(), { status: 'ok' });
  } finally { if (child && child.exitCode === null) { child.kill(); await new Promise(r => child.once('exit', r)); } rmSync(dir, { recursive: true, force: true }); }
});

test('VPS artifacts include UI, persistent secrets, HTTPS ingress and exclude credentials from image context', async () => {
  const dockerfile = readFileSync(join(root, 'Dockerfile'), 'utf8');
  assert.match(dockerfile, /COPY public\/ \.\/public\//);
  assert.match(dockerfile, /USER node/);
  assert.match(dockerfile, /npm ci/);
  const ignore = readFileSync(join(root, '.dockerignore'), 'utf8');
  assert.match(ignore, /^data\/?$/m);
  assert.match(ignore, /^\.env$/m);
  assert.match(ignore, /^node_modules\/?$/m);
  const compose = readFileSync(join(root, 'docker-compose.yml'), 'utf8');
  assert.match(compose, /127\.0\.0\.1.*8080/);
  assert.match(compose, /DATA_DIR:\s*\/app\/data/);
  assert.match(compose, /PUBLIC_URL:\s*\$\{PUBLIC_URL/);
  const caddy = readFileSync(join(root, 'deploy/Caddyfile'), 'utf8');
  assert.match(caddy, /reverse_proxy/);
  assert.match(caddy, /flush_interval -1/);
  const docs = readFileSync(join(root, 'README.md'), 'utf8');
  assert.match(docs, /not.*unlimited/i);
  assert.match(docs, /first visitor/i);
  assert.match(docs, /before.*public/i);
});

test('runtime dependency audit reports zero high or critical advisories', async () => {
  const executable = process.platform === 'win32' ? 'cmd.exe' : 'npm';
  const args = process.platform === 'win32' ? ['/d', '/s', '/c', 'npm audit --omit=dev --json'] : ['audit', '--omit=dev', '--json'];
  const child = spawn(executable, args, { cwd: root });
  let output = ''; child.stdout.on('data', data => { output += data; });
  await new Promise((resolve, reject) => { child.once('error', reject); child.once('exit', resolve); });
  const data = JSON.parse(output);
  assert.equal(data.metadata.vulnerabilities.high, 0);
  assert.equal(data.metadata.vulnerabilities.critical, 0);
});
