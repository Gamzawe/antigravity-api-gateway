import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { createHash, randomBytes } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { fileURLToPath } from 'node:url';

export const root = fileURLToPath(new URL('../', import.meta.url));
export const evidenceDir = resolve(root, '.unlazy/owner-dashboard/evidence');
export const fingerprint = file => createHash('sha256').update(readFileSync(file)).digest('hex');
export const newPassword = () => randomBytes(30).toString('base64url');

export async function fixture({ live = false } = {}) {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-console-check-'));
  const source = resolve(root, 'data/accounts.json');
  const before = live ? fingerprint(source) : null;
  if (live) {
    const data = JSON.parse(readFileSync(source, 'utf8'));
    writeFileSync(join(dir, 'accounts.json'), JSON.stringify(data), { mode: 0o600 });
  } else writeFileSync(join(dir, 'accounts.json'), '{"accounts":[]}');
  const reservation = createServer();
  await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve));
  const port = reservation.address().port;
  await new Promise(resolve => reservation.close(resolve));
  const origin = `http://127.0.0.1:${port}`;
  const child = spawn(process.execPath, [resolve(root, 'src/index.js')], { cwd: root, env: { ...process.env, NODE_ENV: 'test', PORT: String(port), HOST: '127.0.0.1', PUBLIC_URL: origin, DATA_DIR: dir, ACCOUNTS_FILE: join(dir, 'accounts.json'), API_KEYS: '', API_KEY: '', FALLBACK: 'false', DEBUG: 'false' }, stdio: ['ignore', 'pipe', 'pipe'] });
  // Logs may contain upstream account identifiers. Keep them in memory, not transcripts.
  let log = ''; child.stdout.on('data', data => { log += data; }); child.stderr.on('data', data => { log += data; });
  try {
    const deadline = Date.now() + 10000;
    let ready;
    while (Date.now() < deadline) {
      ready = log.split('\n').map(line => { try { return JSON.parse(line); } catch { return null; } }).find(item => item?.ready);
      if (ready) break;
      if (child.exitCode !== null) throw new Error('Test server failed to start.');
      await new Promise(resolve => setTimeout(resolve, 40));
    }
    if (!ready) throw new Error('Test server startup deadline.');
    // Origin is fixed by configuration; actual request URL can use the assigned loopback port.
    const base = `http://127.0.0.1:${ready.port}`;
    return { dir, base, origin, child, before, source, async close() {
      if (child.exitCode === null) { child.kill(); await new Promise(resolve => child.once('exit', resolve)); }
      const unchanged = !live || fingerprint(source) === before;
      rmSync(dir, { recursive: true, force: true });
      if (!unchanged) throw new Error('Source account export changed during qualification.');
    } };
  } catch (error) { child.kill(); rmSync(dir, { recursive: true, force: true }); throw error; }
}
