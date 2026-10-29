import express from 'express';
import { timingSafeEqual, createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const safeEqual = (a, b) => typeof a === 'string' && typeof b === 'string' && timingSafeEqual(createHash('sha256').update(a).digest(), createHash('sha256').update(b).digest());
const fail = (res, status, message) => res.status(status).json({ error: { message } });
const cookieToken = (req, name) => {
  const raw = (req.headers.cookie || '').split(';').find(part => part.trim().startsWith(name + '='));
  return raw ? raw.trim().slice(name.length + 1) : undefined;
};

export function consoleSecurity(req, res, next) {
  res.set({
    'Cache-Control': 'no-store',
    'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
    'X-Content-Type-Options': 'nosniff',
    'X-Frame-Options': 'DENY',
    'Referrer-Policy': 'no-referrer',
    'Permissions-Policy': 'camera=(), microphone=(), geolocation=()'
  });
  next();
}

export function ownerGuard({ store, publicUrl }) {
  const origin = new URL(publicUrl).origin;
  const cookieName = origin.startsWith('https:') ? '__Host-agw_session' : 'agw_session';
  return (req, res, next) => {
    const session = store.session(cookieToken(req, cookieName));
    if (!session) return fail(res, 401, 'Sign in to your private console.');
    if (!['GET', 'HEAD', 'OPTIONS'].includes(req.method) && (req.headers.origin !== origin || !safeEqual(req.headers['x-csrf-token'], session.csrf))) return fail(res, 403, 'Invalid origin or CSRF token. Reload and try again.');
    req.ownerSession = session;
    next();
  };
}

export function createConsole({ store, publicUrl, models = async () => ({ data: [] }) }) {
  const router = express.Router();
  const origin = new URL(publicUrl).origin;
  const secure = origin.startsWith('https:');
  const cookieName = secure ? '__Host-agw_session' : 'agw_session';
  const privateDir = fileURLToPath(new URL('../../public/', import.meta.url));
  const requireOwner = ownerGuard({ store, publicUrl });
  const cookieOptions = { httpOnly: true, sameSite: 'strict', secure, path: '/', maxAge: store.sessionMs };
  let attempts = [];
  const limited = (req, res, next) => {
    const now = Date.now();
    attempts = attempts.filter(time => time > now - 15 * 60 * 1000);
    if (attempts.length >= 8) { res.set('Retry-After', '900'); return fail(res, 429, 'Too many sign-in attempts. Try again in 15 minutes.'); }
    attempts.push(now);
    next();
  };
  const wrap = fn => (req, res, next) => Promise.resolve(fn(req, res, next)).catch(next);
  router.use(consoleSecurity);
  router.use('/api', (req, res, next) => {
    if (!['GET', 'HEAD', 'OPTIONS'].includes(req.method) && req.headers.origin !== origin) return fail(res, 403, 'Invalid request origin.');
    next();
  });
  router.use('/api', express.json({ limit: '16kb', inflate: false, strict: true }));
  router.get('/api/session', (req, res) => {
    const session = store.session(cookieToken(req, cookieName));
    res.json({ configured: store.isConfigured(), authenticated: Boolean(session), ...(session ? { username: session.username, csrf: session.csrf, apiBase: origin + '/v1' } : {}) });
  });
  router.post('/api/setup', limited, wrap(async (req, res) => {
    await store.setup(req.body);
    res.status(201).json({ configured: true });
  }));
  router.post('/api/login', limited, wrap(async (req, res) => {
    const session = await store.login(req.body?.username, req.body?.password);
    store.logout(cookieToken(req, cookieName));
    attempts = [];
    res.cookie(cookieName, session.token, cookieOptions);
    res.json({ authenticated: true, username: session.username, csrf: session.csrf, apiBase: origin + '/v1' });
  }));
  router.post('/api/logout', requireOwner, (req, res) => {
    store.logout(cookieToken(req, cookieName));
    res.clearCookie(cookieName, { ...cookieOptions, maxAge: undefined });
    res.json({ authenticated: false });
  });
  router.get('/api/keys', requireOwner, (req, res) => res.json({ keys: store.listKeys() }));
  router.post('/api/keys', requireOwner, (req, res, next) => {
    try { res.status(201).json(store.createKey(req.body?.name)); } catch (error) { next(error); }
  });
  router.delete('/api/keys/:id', requireOwner, (req, res, next) => {
    try { res.json(store.revokeKey(req.params.id)); } catch (error) { next(error); }
  });
  router.get('/api/models', requireOwner, wrap(async (req, res) => {
    try { res.json(await models()); } catch { fail(res, 502, 'Unable to retrieve upstream models. Check the connected accounts on your server.'); }
  }));
  router.use('/api', (req, res) => fail(res, 404, 'Console endpoint not found.'));
  router.use(express.static(privateDir, { etag: false, lastModified: false, index: 'index.html' }));
  router.use((error, req, res, next) => {
    if (res.headersSent) return next(error);
    const status = error.status >= 400 && error.status < 500 ? error.status : 500;
    fail(res, status, status === 500 ? 'The private console could not complete this action.' : error.type === 'entity.parse.failed' ? 'Invalid JSON.' : error.message);
  });
  return router;
}
