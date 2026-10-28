import test from 'node:test';
import assert from 'node:assert/strict';
import { streamSSEResponse } from '../../src/cloudcode/sse-streamer.js';
import { parseThinkingSSEResponse } from '../../src/cloudcode/sse-parser.js';

test('WP-03: streaming parser rejects incomplete/unterminated SSE frame at EOF with error', async () => {
    // Stream terminates with trailing partial data: { "resp... without newline
    const partialStream = new ReadableStream({
        start(controller) {
            const encoder = new TextEncoder();
            controller.enqueue(encoder.encode('data: {"incomplete": true'));
            controller.close();
        }
    });

    const response = new Response(partialStream, { headers: { 'content-type': 'text/event-stream' } });
    let errorThrown = false;
    try {
        for await (const event of streamSSEResponse(response, 'gemini-3.8-flash')) {
            // should not yield success
        }
    } catch (err) {
        errorThrown = true;
        assert.ok(err.message.includes('Incomplete SSE stream frame at EOF'), 'Error must specify incomplete frame');
    }
    assert.ok(errorThrown, 'Must throw error on unterminated EOF buffer');
});

test('WP-03: non-streaming thinking parser rejects incomplete/unterminated SSE frame at EOF', async () => {
    const partialStream = new ReadableStream({
        start(controller) {
            const encoder = new TextEncoder();
            controller.enqueue(encoder.encode('data: {"truncated":'));
            controller.close();
        }
    });

    const response = new Response(partialStream, { headers: { 'content-type': 'text/event-stream' } });
    let errorThrown = false;
    try {
        await parseThinkingSSEResponse(response, 'gemini-3.8-flash');
    } catch (err) {
        errorThrown = true;
        assert.ok(err.message.includes('Incomplete SSE stream frame at EOF'), 'Error must specify incomplete frame');
    }
    assert.ok(errorThrown, 'Must throw error on unterminated EOF buffer');
});
