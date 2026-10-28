import test from 'node:test';
import assert from 'node:assert/strict';
import { streamSSEResponse } from '../../src/cloudcode/sse-streamer.js';
import { convertAnthropicEventToOpenAI } from '../../src/format/openai-compat.js';

test('WP-02: streamed tool call candidate with upstream STOP finishReason emits tool_calls finish_reason', async () => {
    // Simulated Google CloudCode SSE payload where candidate has a functionCall AND finishReason is STOP
    const googlePayload = JSON.stringify({
        response: {
            candidates: [{
                content: {
                    parts: [{
                        functionCall: {
                            name: 'bash',
                            args: { command: 'ls -la' }
                        }
                    }]
                },
                finishReason: 'STOP'
            }],
            usageMetadata: { promptTokenCount: 15, candidatesTokenCount: 10 }
        }
    });

    const stream = new ReadableStream({
        start(controller) {
            const encoder = new TextEncoder();
            controller.enqueue(encoder.encode(`data: ${googlePayload}\n\n`));
            controller.close();
        }
    });

    const response = new Response(stream, { headers: { 'content-type': 'text/event-stream' } });
    const events = [];
    for await (const event of streamSSEResponse(response, 'gemini-3.8-flash')) {
        events.push(event);
    }

    // Verify Anthropic stop_reason is 'tool_use'
    const messageDelta = events.find(e => e.type === 'message_delta');
    assert.ok(messageDelta, 'Must yield message_delta event');
    assert.equal(messageDelta.delta.stop_reason, 'tool_use', 'Anthropic stop_reason must remain tool_use despite upstream STOP');

    // Convert via OpenAI compat converter
    const state = {};
    const openAiChunks = [];
    for (const event of events) {
        const chunk = convertAnthropicEventToOpenAI(event, 'gemini-3.8-flash', state);
        if (chunk) openAiChunks.push(chunk);
    }

    const finalChunk = openAiChunks.find(c => c.choices?.[0]?.finish_reason !== null);
    assert.ok(finalChunk, 'Must have a chunk with finish_reason');
    assert.equal(finalChunk.choices[0].finish_reason, 'tool_calls', 'OpenAI finish_reason must be tool_calls');
});
