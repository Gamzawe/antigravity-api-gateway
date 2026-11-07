// WP-14: Client Integration & Thinking Profile Validation
// Validates the deployed gateway end-to-end with the payload shapes that
// OpenCode and Hermes send: JSON-Schema tool declarations (including
// $schema / exclusiveMinimum / exclusiveMaximum), non-streaming and
// streaming tool execution, and a plain chat sanity check.
//
// Requires client-approval. Run with:
//   GATEWAY_URL=https://... GATEWAY_API_KEY=sk-... \
//     node --test tests/integration/client-compatibility.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';

const BASE_URL = (process.env.GATEWAY_URL || 'https://antigravity-api-gateway.onrender.com').replace(/\/$/, '');
const API_KEY = process.env.GATEWAY_API_KEY || '';
const MODEL = process.env.GATEWAY_MODEL || 'gemini-3.8-flash:low';
const DEPRECATION = 'no longer supported';

const toolSchema = {
    $schema: 'http://json-schema.org/draft-07/schema#',
    type: 'object',
    additionalProperties: false,
    properties: {
        city: { type: 'string', format: 'uri' },
        days: { type: 'integer', exclusiveMinimum: 0, exclusiveMaximum: 15 }
    },
    required: ['city']
};

const tools = [{
    type: 'function',
    function: { name: 'get_weather', description: 'Get weather', parameters: toolSchema }
}];

function headers() {
    return { Authorization: `Bearer ${API_KEY}`, 'Content-Type': 'application/json' };
}

const skip = API_KEY ? false : 'GATEWAY_API_KEY not set (WP-14 requires client-approval)';

test('WP-14: non-streaming tool call with draft-07 schema keywords', { skip }, async () => {
    const res = await fetch(`${BASE_URL}/v1/chat/completions`, {
        method: 'POST',
        headers: headers(),
        body: JSON.stringify({
            model: MODEL,
            messages: [{ role: 'user', content: 'What is the weather in Paris? Use the get_weather tool.' }],
            tools,
            max_tokens: 200
        })
    });
    assert.equal(res.status, 200, `expected 200, got ${res.status}`);
    const body = await res.json();
    assert.ok(!JSON.stringify(body).includes(DEPRECATION), 'upstream client-version deprecation leaked');
    const choice = body.choices?.[0];
    assert.ok(choice, 'missing choice');
    assert.equal(choice.finish_reason, 'tool_calls');
    const call = choice.message?.tool_calls?.[0];
    assert.ok(call, 'missing tool_calls');
    assert.equal(call.function.name, 'get_weather');
    assert.ok(call.function.arguments.includes('Paris'));
});

test('WP-14: streaming tool call emits terminal finish_reason and [DONE]', { skip }, async () => {
    const res = await fetch(`${BASE_URL}/v1/chat/completions`, {
        method: 'POST',
        headers: headers(),
        body: JSON.stringify({
            model: MODEL,
            stream: true,
            messages: [{ role: 'user', content: 'Use get_weather for Tokyo.' }],
            tools,
            max_tokens: 150
        })
    });
    assert.equal(res.status, 200, `expected 200, got ${res.status}`);
    const text = await res.text();
    assert.ok(!text.includes(DEPRECATION), 'upstream client-version deprecation leaked');
    assert.ok(text.includes('"finish_reason":"tool_calls"'), 'missing tool_calls finish reason');
    assert.ok(text.includes('data: [DONE]'), 'missing [DONE] terminator');
});

test('WP-14: plain chat returns real assistant content', { skip }, async () => {
    const res = await fetch(`${BASE_URL}/v1/chat/completions`, {
        method: 'POST',
        headers: headers(),
        body: JSON.stringify({
            model: MODEL,
            messages: [{ role: 'user', content: 'Reply with exactly: PONG' }],
            max_tokens: 20
        })
    });
    assert.equal(res.status, 200, `expected 200, got ${res.status}`);
    const body = await res.json();
    const content = body.choices?.[0]?.message?.content || '';
    assert.ok(content.trim().length > 0, 'empty assistant content');
    assert.ok(!content.includes(DEPRECATION), 'upstream client-version deprecation leaked');
});
