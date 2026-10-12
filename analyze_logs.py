
import sys
import time
from collections import Counter
from pathlib import Path


def analyze_logs(filename):
    total = 0
    success = 0
    client_errors = 0
    server_errors = 0
    total_duration = 0

    endpoints = Counter()

    with open(filename, "r") as file:
        for line in file:
            parts = line.split()

            if len(parts) != 6:
                continue

            timestamp, ip, method, path, status, duration = parts

            status = int(status)
            duration = int(duration.removesuffix("ms"))

            total += 1
            total_duration += duration
            endpoints[path] += 1

            if 200 <= status < 300:
                success += 1
            elif 400 <= status < 500:
                client_errors += 1
            elif 500 <= status < 600:
                server_errors += 1

    return {
        "total": total,
        "success": success,
        "client_errors": client_errors,
        "server_errors": server_errors,
        "average_ms": total_duration / total if total else 0,
        "top_endpoints": endpoints.most_common(5),
    }


if __name__ == "__main__":
    filename = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("data/server.log")

    start = time.perf_counter()
    results = analyze_logs(filename)
    elapsed = time.perf_counter() - start

    print("\n=== Server Log Analysis ===")
    print(f"Total requests: {results['total']:,}")
    print(f"Successful: {results['success']:,}")
    print(f"Client errors: {results['client_errors']:,}")
    print(f"Server errors: {results['server_errors']:,}")
    print(f"Average response time: {results['average_ms']:.2f} ms")

    print("\nTop endpoints:")
    for path, count in results["top_endpoints"]:
        print(f"  {path}: {count:,}")

    print(f"\nExecution time: {elapsed:.4f} seconds")

