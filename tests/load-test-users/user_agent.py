"""
User Agent Simulator
Simulates a real human user interacting with an autonomous Hermes agent.
Each session executes a multi-turn conversation:
1. User prompt -> Hermes responds with a tool call
2. Simulated tool execution -> Hermes synthesizes the final reply
3. Human reading pause (randomized delay)
"""

import asyncio
import random
import time
from payloads import HERMES_SYSTEM_PROMPT, HERMES_TOOLS, USER_SCENARIOS


async def simulate_hermes_user(
    client,
    base_url: str,
    api_key: str,
    user_id: int,
    stats,
    duration_seconds: int,
    model: str = "gemini-3.8-flash"
):
    """
    Simulates a single active user interacting with their Hermes backend.
    """
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}"
    }

    start_time = time.time()
    turn_counter = 0

    while time.time() - start_time < duration_seconds:
        turn_counter += 1
        scenario = random.choice(USER_SCENARIOS)

        # Step 1: User sends initial task prompt
        messages = [
            {"role": "system", "content": HERMES_SYSTEM_PROMPT},
            {"role": "user", "content": scenario["task"]}
        ]

        # Call Hermes
        t0 = time.time()
        try:
            req_payload = {
                "model": model,
                "messages": messages,
                "tools": HERMES_TOOLS,
                "max_tokens": 100
            }
            resp = await client.post(
                f"{base_url}/v1/chat/completions",
                headers=headers,
                json=req_payload,
                timeout=45.0
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

            # Step 2: Inject simulated tool result and ask for final answer
            messages.append({
                "role": "assistant",
                "content": f"I will check this using {scenario['simulated_tool']}."
            })
            messages.append({
                "role": "user",
                "content": f"Tool result:\n{scenario['tool_result']}\nPlease summarize the findings."
            })

            t1 = time.time()
            resp2 = await client.post(
                f"{base_url}/v1/chat/completions",
                headers=headers,
                json={
                    "model": model,
                    "messages": messages,
                    "max_tokens": 80
                },
                timeout=45.0
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

        # Step 3: Human thinking / reading pause (realistic: 10s to 30s)
        # For accelerated load testing, use a realistic 5s to 15s pause
        pause = random.uniform(5.0, 15.0)
        await asyncio.sleep(pause)
