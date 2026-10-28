"""
Exact Hermes Virtual User Simulator
Executes multi-turn agent conversations with the FULL 25,000+ character system prompt,
active skills, and tool declarations.
"""

import asyncio
import random
import time
from payloads import EXACT_HERMES_SYSTEM_PROMPT, EXACT_HERMES_TOOLS, REAL_CONVERSATION_SCENARIOS


async def simulate_exact_hermes_user(
    client,
    base_url: str,
    api_key: str,
    user_id: int,
    stats,
    duration_seconds: int,
    model: str = "gemini-3.8-flash"
):
    """
    Simulates one human user running an actual Hermes instance with their full config.
    """
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}"
    }

    start_time = time.time()

    while time.time() - start_time < duration_seconds:
        scenario = random.choice(REAL_CONVERSATION_SCENARIOS)

        # Build real conversation turn 1 with complete system prompt (25k+ characters)
        messages = [
            {"role": "system", "content": EXACT_HERMES_SYSTEM_PROMPT},
            {"role": "user", "content": scenario["user_turn_1"]}
        ]

        t0 = time.time()
        try:
            req_body = {
                "model": model,
                "messages": messages,
                "tools": EXACT_HERMES_TOOLS,
                "max_tokens": 120
            }
            resp = await client.post(
                f"{base_url}/v1/chat/completions",
                headers=headers,
                json=req_body,
                timeout=60.0
            )
            lat1 = time.time() - t0

            if resp.status_code == 200:
                stats.record_success(lat1)
            elif resp.status_code == 429:
                stats.record_throttle(resp.text)
                await asyncio.sleep(2.0)
                continue
            else:
                stats.record_error(resp.status_code, resp.text)
                await asyncio.sleep(2.0)
                continue

            # Simulate agent executing tool and receiving response
            messages.append({
                "role": "assistant",
                "content": f"I will look this up using `{scenario['simulated_tool']}`."
            })
            messages.append({
                "role": "user",
                "content": f"[Tool Output: {scenario['simulated_tool']}]\n{scenario['tool_response']}\n\n{scenario['user_turn_2']}"
            })

            t1 = time.time()
            resp2 = await client.post(
                f"{base_url}/v1/chat/completions",
                headers=headers,
                json={
                    "model": model,
                    "messages": messages,
                    "max_tokens": 100
                },
                timeout=60.0
            )
            lat2 = time.time() - t1

            if resp2.status_code == 200:
                stats.record_success(lat2)
            elif resp2.status_code == 429:
                stats.record_throttle(resp2.text)
            else:
                stats.record_error(resp2.status_code, resp2.text)

        except asyncio.TimeoutError:
            stats.record_timeout()
        except Exception as e:
            stats.record_error(500, str(e))

        # Real-world human think/read time between turns (8s to 18s)
        pause = random.uniform(8.0, 18.0)
        await asyncio.sleep(pause)
