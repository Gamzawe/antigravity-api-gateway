import os
import asyncio
import httpx

BASE_URL = os.getenv("GATEWAY_URL", "http://127.0.0.1:8089")
API_KEY = os.getenv("PRIVATE_GATEWAY_API_KEY", "")

async def verify_distribution():
    print("Sending 6 concurrent requests to inspect account distribution...")
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {API_KEY}"
    }

    async with httpx.AsyncClient() as client:
        async def call(i):
            resp = await client.post(
                f"{BASE_URL}/v1/chat/completions",
                headers=headers,
                json={
                    "model": "gemini-3.8-flash",
                    "messages": [{"role": "user", "content": f"Ping #{i}. Say 1."}],
                    "max_tokens": 5
                },
                timeout=30.0
            )
            return resp.status_code

        tasks = [call(i) for i in range(1, 7)]
        statuses = await asyncio.gather(*tasks)
        print("Completed 6 calls. Statuses:", statuses)

if __name__ == "__main__":
    asyncio.run(verify_distribution())
