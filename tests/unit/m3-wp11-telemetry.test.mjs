import test from 'node:test';
import assert from 'node:assert/strict';
import { convertAnthropicEventToOpenAI } from '../../src/format/openai-compat.js';

test('WP-11: convertAnthropicEventToOpenAI populates non-zero prompt_tokens and completion_tokens from usage', () => {
    const event = {
        type: 'message_delta',
        delta: { stop_reason: 'end_turn' },
        usage: {
            input_tokens: 42,
            output_tokens: 18,
            cache_read_input_tokens: 5
        }
    };

    const chunk = convertAnthropicEventToOpenAI(event, 'gemini-3.8-flash', {});
    assert.ok(chunk);
    assert.ok(chunk.usage);
    assert.equal(chunk.usage.prompt_tokens, 47, 'Prompt tokens must include cache read tokens when present');
    assert.equal(chunk.usage.completion_tokens, 18, 'Completion tokens must reflect output tokens');
    assert.equal(chunk.usage.total_tokens, 65, 'Total tokens must reflect sum of input, cache and output');
});
