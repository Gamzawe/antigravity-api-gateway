import { test } from 'node:test';
import assert from 'node:assert/strict';
import { convertOpenAIToAnthropic } from '../../src/format/openai-compat.js';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

test('remote converter rejects local filenames even without file URI scheme; inline bytes still work', () => {
  const dir = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'agw-file-'));
  try {
    const path = join(dir, 'private.txt');
    writeFileSync(path, 'PRIVATE_TEST_CONTENT');
    const request = url => ({ model: 'gemini-3.8-flash', messages: [{ role: 'user', content: [{ type: 'file', url, mediaType: 'text/plain' }] }] });
    assert.throws(() => convertOpenAIToAnthropic(request(path)), /inline base64/i, 'converter must never open a client-supplied path');
    assert.throws(() => convertOpenAIToAnthropic(request('file:///etc/passwd')), /inline base64/i);
    const result = convertOpenAIToAnthropic({ model: 'gemini-3.8-flash', messages: [{ role: 'user', content: [{ type: 'file', source: { type: 'base64', media_type: 'text/plain', data: 'aGVsbG8=' } }] }] });
    assert.equal(result.messages[0].content[0].source.data, 'aGVsbG8=');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
