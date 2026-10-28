import { randomBytes, scrypt as scryptCallback, timingSafeEqual, createHash } from 'node:crypto';
import { promisify } from 'node:util';
import { mkdirSync, existsSync, readFileSync, writeFileSync, renameSync, unlinkSync, chmodSync } from 'node:fs';
import { join } from 'node:path';

const scrypt = promisify(scryptCallback);
const random = () => randomBytes(32).toString('base64url');
const digest = text => createHash('sha256').update(text).digest('hex');
const equal = (a, b) => typeof a === 'string' && typeof b === 'string' && timingSafeEqual(Buffer.from(digest(a)), Buffer.from(digest(b)));
const failure = (status, message) => Object.assign(new Error(message), { status });
const defaultNow = () => Date.now();

/** Single-process private state. Never store raw passwords, API keys or sessions. */
export class AccessStore {
  constructor(directory, { now = defaultNow, sessionMs = 12 * 60 * 60 * 1000 } = {}) {
    this.directory = directory;
    this.now = now;
    this.sessionMs = sessionMs;
    this.sessions = new Map();
    this.settingUp = false;
    mkdirSync(directory, { recursive: true, mode: 0o700 });
    this.path = join(directory, 'access.json');
    this.state = existsSync(this.path) ? JSON.parse(readFileSync(this.path, 'utf8')) : { version: 1, owner: null, keys: [] };
    if (this.state.version !== 1 || !Array.isArray(this.state.keys) || (this.state.owner && (!this.state.owner.hash || !this.state.owner.salt || !this.state.owner.username))) {
      throw new Error('Invalid private access state. Refusing to start.');
    }
  }

  isConfigured() { return Boolean(this.state.owner); }

  save() {
    const temp = this.path + '.' + randomBytes(8).toString('hex') + '.tmp';
    try {
      writeFileSync(temp, JSON.stringify(this.state, null, 2), { mode: 0o600, flag: 'wx', flush: true });
      if (process.platform !== 'win32') chmodSync(temp, 0o600);
      renameSync(temp, this.path);
    } finally { if (existsSync(temp)) unlinkSync(temp); }
  }

  async setup({ username, password } = {}) {
    if (this.isConfigured() || this.settingUp) throw failure(409, 'Owner already configured. Sign in instead.');
    if (typeof username !== 'string' || !/^[a-zA-Z0-9@._+-]{1,80}$/.test(username)) throw failure(400, 'Choose a username using letters, numbers, @, dot or dash.');
    if (typeof password !== 'string' || password.length < 12 || password.length > 128) throw failure(400, 'Password must contain 12 to 128 characters.');
    this.settingUp = true;
    try {
      const salt = random();
      const hash = (await scrypt(password, salt, 64, { N: 32768, r: 8, p: 1, maxmem: 64 * 1024 * 1024 })).toString('hex');
      this.state.owner = { username, salt, hash, createdAt: new Date(this.now()).toISOString() };
      try { this.save(); } catch (error) { this.state.owner = null; throw error; }
    } finally { this.settingUp = false; }
  }

  async login(username, password) {
    const owner = this.state.owner;
    if (!owner || typeof password !== 'string' || password.length > 128 || typeof username !== 'string' || username.length > 80) throw failure(401, 'Invalid username or password.');
    const hash = (await scrypt(password, owner.salt, 64, { N: 32768, r: 8, p: 1, maxmem: 64 * 1024 * 1024 })).toString('hex');
    if (!equal(username, owner.username) || !equal(hash, owner.hash)) throw failure(401, 'Invalid username or password.');
    for (const [id, session] of this.sessions) if (session.expiresAt <= this.now()) this.sessions.delete(id);
    if (this.sessions.size >= 20) this.sessions.delete(this.sessions.keys().next().value);
    const token = random();
    const session = { username: owner.username, csrf: random(), expiresAt: this.now() + this.sessionMs };
    this.sessions.set(digest(token), session);
    return { ...session, token };
  }

  session(token) {
    if (typeof token !== 'string' || token.length > 128) return null;
    const session = this.sessions.get(digest(token));
    if (!session) return null;
    if (session.expiresAt <= this.now()) { this.sessions.delete(digest(token)); return null; }
    return { ...session };
  }

  createKey(name) {
    if (typeof name !== 'string' || !name.trim() || name.trim().length > 80) throw failure(400, 'Key name must contain 1 to 80 characters.');
    if (this.state.keys.length >= 200) throw failure(400, 'Key limit reached.');
    const key = 'sk-agw-' + random();
    const record = { id: randomBytes(16).toString('hex'), name: name.trim(), prefix: key.slice(0, 14), hash: digest(key), createdAt: new Date(this.now()).toISOString(), lastUsedAt: null, revokedAt: null };
    this.state.keys.push(record);
    try { this.save(); } catch (error) { this.state.keys.pop(); throw error; }
    const { hash, ...metadata } = record;
    return { ...metadata, key };
  }

  listKeys() { return this.state.keys.map(({ hash, ...metadata }) => ({ ...metadata })); }

  verifyKey(key) {
    if (typeof key !== 'string' || !/^sk-agw-[A-Za-z0-9_-]{43}$/.test(key)) return null;
    const hashed = digest(key);
    const record = this.state.keys.find(item => !item.revokedAt && equal(item.hash, hashed));
    if (!record) return null;
    if (!record.lastUsedAt || this.now() - Date.parse(record.lastUsedAt) >= 60000) {
      const previous = record.lastUsedAt;
      record.lastUsedAt = new Date(this.now()).toISOString();
      try { this.save(); } catch (error) { record.lastUsedAt = previous; throw error; }
    }
    const { hash, ...metadata } = record;
    return { ...metadata };
  }

  revokeKey(id) {
    const record = this.state.keys.find(item => item.id === id);
    if (!record) throw failure(404, 'Key not found.');
    if (!record.revokedAt) {
      const previous = record.revokedAt;
      record.revokedAt = new Date(this.now()).toISOString();
      try { this.save(); } catch (error) { record.revokedAt = previous; throw error; }
    }
    const { hash, ...metadata } = record;
    return { ...metadata };
  }

  logout(token) { if (typeof token === 'string') this.sessions.delete(digest(token)); }
}
