"""
Test Metrics Collector and Formatter
Tracks latency distributions, error counts, throttle spikes, and outputs
clean terminal tables.
"""

import time


class LoadTestStats:
    def __init__(self, target_users: int):
        self.target_users = target_users
        self.start_time = time.time()
        self.successful_requests = 0
        self.throttled_requests = 0
        self.error_requests = 0
        self.timeout_requests = 0
        self.latencies = []
        self.throttle_messages = []

    def record_success(self, latency: float):
        self.successful_requests += 1
        self.latencies.append(latency)

    def record_throttle(self, message: str):
        self.throttled_requests += 1
        self.throttle_messages.append(message[:120])

    def record_error(self, code: int, message: str):
        self.error_requests += 1

    def record_timeout(self):
        self.timeout_requests += 1

    def print_summary(self, label: str):
        elapsed = time.time() - self.start_time
        total_reqs = self.successful_requests + self.throttled_requests + self.error_requests + self.timeout_requests

        p50 = 0.0
        p95 = 0.0
        avg_lat = 0.0
        if self.latencies:
            sorted_lat = sorted(self.latencies)
            p50 = sorted_lat[int(len(sorted_lat) * 0.50)]
            p95 = sorted_lat[int(len(sorted_lat) * 0.95)]
            avg_lat = sum(self.latencies) / len(self.latencies)

        rpm = (self.successful_requests / elapsed) * 60.0 if elapsed > 0 else 0

        print(f"\n=======================================================")
        print(f"  LOAD TEST RESULTS: {label} ({self.target_users} SIMULATED USERS)")
        print(f"=======================================================")
        print(f"  Test Duration:          {elapsed:.1f} seconds")
        print(f"  Simulated Active Users: {self.target_users}")
        print(f"  Total LLM Calls:        {total_reqs}")
        print(f"  Successful (200 OK):    {self.successful_requests}")
        print(f"  Throttled (429):        {self.throttled_requests}")
        print(f"  Errors / Timeouts:      {self.error_requests + self.timeout_requests}")
        print(f"  Effective Throughput:   {rpm:.1f} requests/min")
        print(f"-------------------------------------------------------")
        print(f"  Average Latency:        {avg_lat:.2f}s per turn")
        print(f"  Median Latency (p50):   {p50:.2f}s per turn")
        print(f"  95th Percentile (p95):  {p95:.2f}s per turn")
        print(f"=======================================================")

        if self.throttled_requests == 0 and (self.error_requests + self.timeout_requests) == 0:
            print(f"  STATUS: PASS - Account handled {self.target_users} active users with 100% success!")
        elif self.throttled_requests > 0:
            print(f"  STATUS: CONSTRAINED - Account hit burst rate limits ({self.throttled_requests} throttles).")
        else:
            print(f"  STATUS: DEGRADED - Errors encountered during run.")
        print(f"=======================================================\n")
