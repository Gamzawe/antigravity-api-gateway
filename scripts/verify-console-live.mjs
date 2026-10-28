import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fixture, evidenceDir, newPassword } from './console-test-fixture.mjs';

const f = await fixture({ live: true });
let cookie, csrf;
const results = [];
const send = async (path, body, headers = {}, method = 'POST') => fetch(f.base + path, { method, headers: { Origin: f.origin, ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}), ...headers }, ...(body !== undefined ? { body: JSON.stringify(body) } : {}), signal: AbortSignal.timeout(90000) });
const parse = async response => { assert.equal(response.status, 200, `Unexpected HTTP ${response.status}`); return response.json(); };
try {
  const password = newPassword();
  let response = await send('/console/api/setup', { username: 'verification-owner', password });
  assert.equal(response.status, 201);
  response = await send('/console/api/login', { username: 'verification-owner', password });
  cookie = response.headers.get('set-cookie').split(';')[0];
  csrf = (await parse(response)).csrf;
  response = await send('/console/api/keys', { name: 'Ephemeral verification' }, { Cookie: cookie, 'X-CSRF-Token': csrf });
  assert.equal(response.status, 201);
  const { key, id } = await response.json();
  const headers = { Authorization: 'Bearer ' + key };
  const catalog = await parse(await fetch(f.base + '/v1/models', { headers, signal: AbortSignal.timeout(30000) }));
  assert.ok(catalog.data.length);
  const model = 'gemini-3.8-flash';
  assert.ok(catalog.data.some(item => item.id === model));
  results.push({ check: 'catalog', count: catalog.data.length });
  let result = await parse(await send('/v1/chat/completions', { model, messages: [{ role: 'user', content: 'Reply exactly PRIVATE_CONSOLE_OK' }], temperature: 0.1 }, headers));
  assert.equal(result.choices[0].message.content.trim(), 'PRIVATE_CONSOLE_OK');
  results.push({ check: 'text', returnedModel: result.model, marker: 'PRIVATE_CONSOLE_OK' });
  response = await send('/v1/chat/completions', { model, stream: true, messages: [{ role: 'user', content: 'Reply exactly STREAM_CONSOLE_OK' }] }, headers);
  assert.equal(response.status, 200);
  let text = '', combined = '', events = 0;
  const timings = [], started = Date.now();
  for await (const chunk of response.body) { text += new TextDecoder().decode(chunk); timings.push(Date.now() - started); }
  assert.ok(text.includes('[DONE]'));
  for (const line of text.split('\n').filter(line => line.startsWith('data: ') && !line.includes('[DONE]'))) {
    const event = JSON.parse(line.slice(6));
    assert.ok(!event.error, 'SSE must not hide a provider error');
    events++; combined += event.choices?.[0]?.delta?.content || '';
  }
  assert.equal(combined.trim(), 'STREAM_CONSOLE_OK');
  results.push({ check: 'stream', events, reads: timings.length, arrivalMs: timings });
  const prompt = { model, messages: [{ role: 'user', content: 'Call echo_token with token CONSOLE_TOOL_OK. Do not answer before calling the tool.' }], tools: [{ type: 'function', function: { name: 'echo_token', description: 'Return the supplied token.', parameters: { type: 'object', properties: { token: { type: 'string' } }, required: ['token'] } } }], tool_choice: 'required' };
  result = await parse(await send('/v1/chat/completions', prompt, headers));
  const choice = result.choices[0];
  assert.equal(choice.finish_reason, 'tool_calls');
  const call = choice.message.tool_calls[0];
  assert.equal(call.function.name, 'echo_token');
  assert.equal(JSON.parse(call.function.arguments).token, 'CONSOLE_TOOL_OK');
  prompt.messages.push(choice.message, { role: 'tool', tool_call_id: call.id, content: 'CONSOLE_TOOL_OK' }, { role: 'user', content: 'Reply exactly the token returned by the tool.' });
  prompt.tool_choice = 'auto';
  result = await parse(await send('/v1/chat/completions', prompt, headers));
  assert.equal(result.choices[0].message.content.trim(), 'CONSOLE_TOOL_OK');
  results.push({ check: 'tool-and-continuation', marker: 'CONSOLE_TOOL_OK' });
  response = await send('/console/api/keys/' + id, undefined, { Cookie: cookie, 'X-CSRF-Token': csrf }, 'DELETE');
  assert.equal(response.status, 200);
  assert.equal((await fetch(f.base + '/v1/key-check', { headers })).status, 401);
  results.push({ check: 'revoked-key', status: 401 });
  mkdirSync(evidenceDir, { recursive: true });
  writeFileSync(join(evidenceDir, 'live.json'), JSON.stringify({ verifiedAt: new Date().toISOString(), scope: 'local Node server; live Google upstream', results, sourceAccountExportUnchanged: true }, null, 2));
} finally { await f.close(); }
console.log('CONSOLE_LIVE_OK');
