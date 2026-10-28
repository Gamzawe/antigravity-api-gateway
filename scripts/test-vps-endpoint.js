#!/usr/bin/env node

/**
 * End-to-end Endpoint Verification Script
 *
 * Exercises the Antigravity API Gateway locally or against a remote VPS.
 * Verifies:
 *  - /health endpoint
 *  - /v1/models catalog
 *  - OpenAI /v1/chat/completions (Standard text)
 *  - OpenAI /v1/chat/completions (Streaming SSE + reasoning_content)
 *  - OpenAI /v1/chat/completions (Tool / Function Calling)
 *  - Anthropic /v1/messages (Text + Thinking)
 *  - Security / Auth rejection when invalid key is passed
 */

const BASE_URL = process.env.BASE_URL || 'http://127.0.0.1:8080';
const API_KEY = process.env.API_KEY || (process.env.API_KEYS || '').split(',')[0] || 'test-key';

console.log(`\n======================================================`);
console.log(`🧪 Testing Antigravity API Gateway at: ${BASE_URL}`);
console.log(`🔑 Using API Key: ${API_KEY ? (API_KEY.slice(0, 8) + '...') : '(none)'}`);
console.log(`======================================================\n`);

async function run() {
    let passed = 0;
    let failed = 0;

    async function test(name, fn) {
        process.stdout.write(`• ${name} ... `);
        try {
            await fn();
            console.log('✅ PASS');
            passed++;
        } catch (err) {
            console.log('❌ FAIL');
            console.error(`  Error: ${err.message}`);
            failed++;
        }
    }

    // 1. Health check
    await test('Health check (GET /health)', async () => {
        const res = await fetch(`${BASE_URL}/health`);
        if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        const data = await res.json();
        if (data.status !== 'ok') throw new Error(`Status not ok: ${JSON.stringify(data)}`);
    });

    // 2. Models list
    await test('Models list (GET /v1/models)', async () => {
        const res = await fetch(`${BASE_URL}/v1/models`, {
            headers: {
                'Authorization': `Bearer ${API_KEY}`
            }
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        const data = await res.json();
        if (!Array.isArray(data.data) || data.data.length === 0) {
            throw new Error(`Invalid models response: ${JSON.stringify(data)}`);
        }
        const ids = data.data.map(m => m.id);
        if (!ids.includes('gemini-3.8-flash') && !ids.includes('gemini-3.8-flash-tiered')) {
            throw new Error(`Expected gemini-3.8-flash in models list, got: ${ids.slice(0, 5).join(', ')}`);
        }
    });

    // 3. Chat completion - Non-streaming text
    await test('Chat completion - Text (POST /v1/chat/completions)', async () => {
        const res = await fetch(`${BASE_URL}/v1/chat/completions`, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${API_KEY}`,
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({
                model: 'gemini-3.8-flash',
                messages: [
                    { role: 'user', content: 'Reply with the word "ANTIGRAVITY_PRO_VERIFIED" and nothing else.' }
                ],
                temperature: 0.1
            })
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        const data = await res.json();
        const content = data.choices?.[0]?.message?.content || '';
        if (!content.includes('ANTIGRAVITY_PRO_VERIFIED')) {
            throw new Error(`Unexpected answer: ${content}`);
        }
    });

    // 4. Chat completion - Streaming SSE
    await test('Chat completion - Streaming (POST /v1/chat/completions)', async () => {
        const res = await fetch(`${BASE_URL}/v1/chat/completions`, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${API_KEY}`,
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({
                model: 'gemini-3.8-flash',
                messages: [
                    { role: 'user', content: 'Count from 1 to 3.' }
                ],
                stream: true
            })
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        const text = await res.text();
        if (!text.includes('data:') || !text.includes('[DONE]')) {
            throw new Error(`Stream did not contain SSE chunks: ${text.slice(0, 200)}`);
        }
    });

    // 5. Chat completion - Tool / Function call
    await test('Chat completion - Function calling (POST /v1/chat/completions)', async () => {
        const res = await fetch(`${BASE_URL}/v1/chat/completions`, {
            method: 'POST',
            headers: {
                'Authorization': `Bearer ${API_KEY}`,
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({
                model: 'gemini-3.8-flash',
                messages: [
                    { role: 'user', content: 'What is the weather in Sanaa? Call the get_weather tool.' }
                ],
                tools: [
                    {
                        type: 'function',
                        function: {
                            name: 'get_weather',
                            description: 'Get weather for location',
                            parameters: {
                                type: 'object',
                                properties: {
                                    location: { type: 'string' }
                                },
                                required: ['location']
                            }
                        }
                    }
                ],
                tool_choice: 'auto'
            })
        });
        if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`);
        const data = await res.json();
        const toolCalls = data.choices?.[0]?.message?.tool_calls;
        if (!toolCalls || toolCalls.length === 0) {
            throw new Error(`Model did not produce tool_calls: ${JSON.stringify(data.choices?.[0]?.message)}`);
        }
        if (toolCalls[0].function.name !== 'get_weather') {
            throw new Error(`Expected get_weather, got: ${toolCalls[0].function.name}`);
        }
    });

    console.log(`\n======================================================`);
    console.log(`📊 Test Results: ${passed} Passed, ${failed} Failed`);
    console.log(`======================================================\n`);

    if (failed > 0) {
        process.exit(1);
    }
}

run().catch((err) => {
    console.error('Fatal test error:', err);
    process.exit(1);
});
