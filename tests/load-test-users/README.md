# Hermes Multi-User Load Tester (Axion Mind Simulator)

This Python project empirically load-tests the Antigravity API Gateway to evaluate how many simultaneous active Hermes users a single Google CloudCode / Antigravity account can sustain.

## What It Simulates
Each simulated user behaves like a real person using their personal Hermes agent:
1. **Realistic Prompts:** Sends real coding and analysis questions.
2. **Heavy Agent Payloads:** Transmits full Hermes system prompts, tool schemas (`web_search`, `read_file`, `search_files`), and multi-turn message history.
3. **Multi-Turn Tool Loops:** Simulates Hermes requesting tool calls, receiving injected tool outputs, and generating final synthesized replies.
4. **Human Pacing:** Includes randomized 5–15 second reading and thinking pauses between turns.

## Project Structure
- `run_test.py`: Main orchestrator running stages of 25 and 50 concurrent users.
- `user_agent.py`: Independent virtual user coroutine executing the multi-turn agent loop.
- `payloads.py`: Real Hermes prompt templates and scenario payloads.
- `report.py`: Metric collection (latency percentiles p50/p95, 429 throttle count, error rate).

## How to Run
Run via `uv`:
```bash
uv run --with httpx python tests/load-test-users/run_test.py
```
