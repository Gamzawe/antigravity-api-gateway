"""
Main Entrypoint: Hermes Multi-User Load Test
Tests capacity of a single account under 25 and 50 simulated active Hermes users.
"""

import asyncio
import os
import sys
import httpx
from user_agent import simulate_hermes_user
from report import LoadTestStats

BASE_URL = os.getenv("GATEWAY_URL", "http://127.0.0.1:8089")
API_KEY = os.getenv("PRIVATE_GATEWAY_API_KEY", "")


async def run_stage(user_count: int, duration_seconds: int):
    print(f"\n>>> Starting Stage: {user_count} Simultaneous Active Hermes Users ({duration_seconds}s run)...")
    stats = LoadTestStats(target_users=user_count)

    limits = httpx.Limits(max_keepalive_connections=user_count + 10, max_connections=user_count + 20)
    async with httpx.AsyncClient(limits=limits) as client:
        tasks = []
        for uid in range(1, user_count + 1):
            t = asyncio.create_task(
                simulate_hermes_user(
                    client=client,
                    base_url=BASE_URL,
                    api_key=API_KEY,
                    user_id=uid,
                    stats=stats,
                    duration_seconds=duration_seconds
                )
            )
            tasks.append(t)
            # Stagger launch slightly (0.1s) to avoid unnatural single-microsecond process startup
            await asyncio.sleep(0.1)

        await asyncio.gather(*tasks)

    stats.print_summary(f"{user_count} Users Test")
    return stats


async def main():
    if not API_KEY:
        print("ERROR: PRIVATE_GATEWAY_API_KEY environment variable is required.")
        sys.exit(1)

    print("==================================================================")
    print("      HERMES MULTI-USER LOAD TEST (SINGLE-ACCOUNT EXPERIMENT)     ")
    print("==================================================================")
    print(f"Target Gateway: {BASE_URL}")
    print(f"Target Model:   gemini-3.8-flash")
    print("Testing realistic multi-turn tool loops with full Hermes payloads.")

    # Phase 1: Test 25 Active Users for 45 seconds
    stats_25 = await run_stage(user_count=25, duration_seconds=45)

    # Cooldown between stages
    print("Cooling down 10 seconds before next stage...")
    await asyncio.sleep(10.0)

    # Phase 2: Test 50 Active Users for 45 seconds
    stats_50 = await run_stage(user_count=50, duration_seconds=45)


if __name__ == "__main__":
    asyncio.run(main())
