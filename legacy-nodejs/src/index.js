import 'dotenv/config';

// Environment is loaded before the server and its module-level constants.
const host = process.env.HOST || '127.0.0.1';
const port = Number(process.env.PORT || 8080);
const publicUrl = process.env.PUBLIC_URL || `http://localhost:${port}`;
let url;
try { url = new URL(publicUrl); } catch { console.error('PUBLIC_URL_INVALID'); process.exit(1); }
if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash || url.pathname !== '/') { console.error('PUBLIC_URL_INVALID: use an origin only.'); process.exit(1); }
if (url.protocol !== 'https:' && (process.env.NODE_ENV === 'production' || !['localhost', '127.0.0.1', '[::1]'].includes(url.hostname))) { console.error('HTTPS_REQUIRED: use PUBLIC_URL=https://your-domain'); process.exit(1); }
if (!Number.isInteger(port) || port < 0 || port > 65535) { console.error('PORT_INVALID'); process.exit(1); }
process.env.PUBLIC_URL = url.origin;
const { default: app } = await import('./server.js');
const { logger } = await import('./utils/logger.js');
logger.setDebug(process.env.DEBUG === 'true' || process.argv.includes('--debug'));
const server = app.listen(port, host, () => {
  console.log(JSON.stringify({ ready: true, host, port: server.address().port, console: url.origin + '/console/' }));
});
server.on('error', error => { console.error('LISTEN_FAILED: ' + error.code); process.exitCode = 1; });
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => {
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(1), 10000).unref();
});
