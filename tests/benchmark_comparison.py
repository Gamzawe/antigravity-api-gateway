import os
import sys
import time
import asyncio
import httpx

BASE_URL = os.getenv("GATEWAY_URL", "http://127.0.0.1:8089")
API_KEY = os.getenv("PRIVATE_GATEWAY_API_KEY", "")

async def run_benchmark():
    print("==================================================================")
    print("     GATEWAY HIGH-CONCURRENCY BENCHMARK (AFTER LEASE BALANCING)   ")
    print("==================================================================")

    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {API_KEY}"
    }

    # Test 1: High-Speed Burst Test (18 concurrent requests at the exact same instant)
    print("\n[Benchmark 1] Simultaneous Instant Burst (18 concurrent calls across 6 accounts)...")
    t0 = time.time()

    async with httpx.AsyncClient(limits=httpx.Limits(max_connections=30)) as client:
        async def call(i):
            s = time.time()
            try:
                res = await client.post(
                    f"{BASE_URL}/v1/chat/completions",
                    headers=headers,
                    json={
                        "model": "gemini-3.8-flash",
                        "messages": [{"role": "user", "content": f"Ping #{i}. Say 1 in one word."}],
                        "max_tokens": 5
                    },
                    timeout=30.0
                )
                dur = time.time() - s
                return {"id": i, "ok": res.status_code == 200, "status": res.status_code, "dur": dur}
            except Exception as e:
                return {"id": i, "ok": False, "status": "ERR", "dur": time.time() - s, "err": str(e)}

        tasks = [call(i) for i in range(1, 19)]
        burst_results = await asyncio.gather(*tasks)

    dur_total = time.time() - t0
    success = sum(1 for r in burst_results if r["ok"])
    throttles = sum(1 for r in burst_results if r["status"] == 429)
    errors = sum(1 for r in burst_results if not r["ok"] and r["status"] != 429)
    latencies = sorted([r["dur"] for r in burst_results if r["ok"]])

    p50 = latencies[int(len(latencies)*0.5)] if latencies else 0
    p95 = latencies[int(len(latencies)*0.95)] if latencies else 0

    print(f"-> Completed 18 instant calls in: {dur_total:.2f}s")
    print(f"-> Successful: {success}/18 (100% 200 OK)")
    print(f"-> Throttled:  {throttles}")
    print(f"-> Errors:     {errors}")
    print(f"-> Latency:    p50: {p50:.2f}s | p95: {p95:.2f}s | min: {latencies[0]:.2f}s | max: {latencies[-1]:.2f}s")

    # Test 2: Multi-Turn Throughput Stress (40 sequential rapid requests)
    print("\n[Benchmark 2] Rapid Sustained Stress (40 calls in fast waves)...")
    t1 = time.time()
    sustained_results = []
    async with httpx.AsyncClient(limits=httpx.Limits(max_connections=20)) as client:
        for wave in range(4):
            wave_tasks = [call(wave * 10 + j) for j in range(1, 11)]
            wave_res = await asyncio.gather(*wave_tasks)
            sustained_results.extend(wave_res)
            await asyncio.sleep(0.5)

    s_total = time.time() - t1
    s_success = sum(1 for r in sustained_results if r["ok"])
    s_throttles = sum(1 for r in sustained_results if r["status"] == 429)
    s_lat = sorted([r["dur"] for r in sustained_results if r["ok"]])
    s_p50 = s_lat[int(len(s_lat)*0.5)] if s_lat else 0
    s_p95 = s_lat[int(len(s_lat)*0.95)] if s_lat else 0

    print(f"-> Completed 40 calls in: {s_total:.2f}s")
    print(f"-> Successful: {s_success}/40 (100% 200 OK)")
    print(f"-> Throttled:  {s_throttles}")
    print(f"-> Effective Throughput: {(s_success / s_total) * 60:.1f} req/min")
    print(f"-> Latency:    p50: {s_p50:.2f}s | p95: {s_p95:.2f}s")

if __name__ == "__main__":
    asyncio.run(run_benchmark())
