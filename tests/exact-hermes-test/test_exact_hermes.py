"""
Exact Hermes Stress Runner with Quota Fraction Tracking
Ramps simulated users in stepped waves and tracks quota consumption.
"""

import asyncio
import os
import sys
import httpx
from user_agent import simulate_exact_hermes_user
from payloads import EXACT_HERMES_SYSTEM_PROMPT

BASE_URL = os.getenv("GATEWAY_URL", "http://127.0.0.1:8089")
API_KEY = os.getenv("PRIVATE_GATEWAY_API_KEY", "")


class ExactTestStats:
    def __init__(self, target_users: int):
        self.target_users = target_users
        self.start_time = time.time()
        self.successful = 0
        self.throttled = 0
        self.errors = 0
        self.timeouts = 0
        self.latencies = []

    def record_success(self, latency: float):
        self.successful += 1
        self.latencies.append(latency)

    def record_throttle(self, msg: str):
        self.throttled += 1

    def record_error(self, code: int, msg: str):
        self.errors += 1

    def record_timeout(self):
        self.timeouts += 1

    def report(self, stage_name: str):
        elapsed = time.time() - self.start_time
        total = self.successful + self.throttled + self.errors + self.timeouts

        p50 = 0.0
        p95 = 0.0
        avg = 0.0
        if self.latencies:
            sorted_lat = sorted(self.latencies)
            p50 = sorted_lat[int(len(sorted_lat) * 0.50)]
            p95 = sorted_lat[int(len(sorted_lat) * 0.95)]
            avg = sum(self.latencies) / len(self.latencies)

        rpm = (self.successful / elapsed) * 60.0 if elapsed > 0 else 0

        print(f"\n=======================================================")
        print(f"  STAGE RESULTS: {stage_name} ({self.target_users} EXACT HERMES USERS)")
        print(f"=======================================================")
        print(f"  Stage Duration:         {elapsed:.1f}s")
        print(f"  Simulated Active Users: {self.target_users}")
        print(f"  Total Large LLM Calls:  {total}")
        print(f"  Successful (200 OK):    {self.successful}")
        print(f"  Throttled (429):        {self.throttled}")
        print(f"  Errors / Timeouts:      {self.errors + self.timeouts}")
        print(f"  Throughput:             {rpm:.1f} requests/min")
        print(f"-------------------------------------------------------")
        print(f"  Avg Latency:            {avg:.2f}s per turn")
        print(f"  Median Latency (p50):   {p50:.2f}s per turn")
        print(f"  95th Percentile (p95):  {p95:.2f}s per turn")
        print(f"=======================================================")
        if self.throttled == 0 and (self.errors + self.timeouts) == 0:
            print(f"  OUTCOME: SUCCESS - Handled with zero throttles or errors.")
        else:
            print(f"  OUTCOME: THROTTLED / CONSTRAINED ({self.throttled} 429 backoffs).")
        print(f"=======================================================\n")


import time


async def run_stage(user_count: int, duration_seconds: int):
    print(f"\n>>> Launching Stage: {user_count} Simultaneous Active Hermes Users ({duration_seconds}s)...")
    stats = ExactTestStats(target_users=user_count)

    limits = httpx.Limits(max_keepalive_connections=user_count + 15, max_connections=user_count + 30)
    async with httpx.AsyncClient(limits=limits) as client:
        tasks = []
        for uid in range(1, user_count + 1):
            t = asyncio.create_task(
                simulate_exact_hermes_user(
                    client=client,
                    base_url=BASE_URL,
                    api_key=API_KEY,
                    user_id=uid,
                    stats=stats,
                    duration_seconds=duration_seconds
                )
            )
            tasks.append(t)
            await asyncio.sleep(0.08)

        await asyncio.gather(*tasks)

    stats.report(f"{user_count}-User Wave")
    return stats


async def main():
    if not API_KEY:
        print("ERROR: PRIVATE_GATEWAY_API_KEY environment variable is required.")
        sys.exit(1)

    print("==================================================================")
    print("   EXACT HERMES CAPACITY TEST (REAL SYSTEM PROMPT & TOOLS)       ")
    print("==================================================================")
    print(f"Base URL:         {BASE_URL}")
    print(f"Model:            gemini-3.8-flash")
    print(f"Prompt Size:      {len(EXACT_HERMES_SYSTEM_PROMPT)} characters (~{int(len(EXACT_HERMES_SYSTEM_PROMPT)/3.5)} tokens)")
    print(f"Tool Schemas:     8 full tool definitions active")
    print("==================================================================")

    # Wave 1: 15 Users (Moderate load)
    await run_stage(user_count=15, duration_seconds=35)

    print("Pausing 5 seconds...")
    await asyncio.sleep(5.0)

    # Wave 2: 30 Users (Target threshold)
    await run_stage(user_count=30, duration_seconds=35)

    print("Pausing 5 seconds...")
    await asyncio.sleep(5.0)

    # Wave 3: 50 Users (Max peak stress)
    await run_stage(user_count=50, duration_seconds=35)


if __name__ == "__main__":
    asyncio.run(main())
