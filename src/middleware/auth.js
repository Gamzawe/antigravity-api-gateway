import { timingSafeEqual, createHash } from 'node:crypto';
const hash = value => createHash('sha256').update(value).digest();

export function createAuthMiddleware(store) {
  return function authMiddleware(req, res, next) {
    if (req.ownerSession) return next();
    const bearer = /^Bearer\s+([^\s]+)$/i.exec(req.headers.authorization || '')?.[1];
    const token = bearer || (typeof req.headers['x-api-key'] === 'string' ? req.headers['x-api-key'] : '');
    const legacy = (process.env.API_KEYS || process.env.API_KEY || '').split(',').map(value => value.trim()).filter(Boolean);
    const configured = token && token.length <= 256 && legacy.some(value => timingSafeEqual(hash(value), hash(token)));
    try {
      const key = token && !configured ? store.verifyKey(token) : null;
      if (configured || key) { req.apiKeyId = key?.id || 'environment'; return next(); }
    } catch { return res.status(503).json({ error: { type: 'api_error', message: 'API access store unavailable.' } }); }
    res.status(401).json({ error: { type: 'authentication_error', code: 'invalid_api_key', message: 'Provide a valid API key using Authorization: Bearer or x-api-key.' } });
  };
}
