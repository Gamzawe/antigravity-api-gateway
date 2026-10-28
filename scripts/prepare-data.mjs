import { lstat, readdir, mkdir, chown, chmod } from 'node:fs/promises';
import { resolve, join } from 'node:path';

/** One-shot container initializer, never part of the unprivileged API process. */
async function prepare(directory) {
  if (process.platform !== 'linux' || process.getuid?.() !== 0) throw new Error('Run the data-init container as root on Linux.');
  const root = resolve(directory || '/app/data');
  if (['/', '/app', '/home', '/etc', '/usr', '/var', '/tmp'].includes(root)) throw new Error('Refusing a broad system directory.');
  await mkdir(root, { recursive: true, mode: 0o700 });
  const items = [];
  const pending = [root];
  // Validate every entry before changing any ownership or mode. Do not follow links.
  while (pending.length) {
    const path = pending.pop();
    const stat = await lstat(path);
    if (!stat.isFile() && !stat.isDirectory()) throw new Error('Private data must contain only regular files and directories; links are refused.');
    items.push({ path, directory: stat.isDirectory() });
    if (items.length > 10000) throw new Error('Private data directory exceeds initialization limit.');
    if (stat.isDirectory()) for (const name of await readdir(path)) pending.push(join(path, name));
  }
  for (const item of items) {
    await chown(item.path, 1000, 1000);
    await chmod(item.path, item.directory ? 0o700 : 0o600);
  }
}
await prepare(process.argv[2]);
console.log('PRIVATE_DATA_READY');
