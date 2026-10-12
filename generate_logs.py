
import random
import sys
from datetime import datetime, timedelta
from pathlib import Path

OUTPUT = Path("data/server.log")

def generate_logs(count):
    OUTPUT.parent.mkdir(exist_ok=True)

    random.seed(42)
    start = datetime(2026, 10, 1)

    paths = ["/", "/login", "/api/users", "/api/products", "/checkout"]
    statuses = [200, 200, 200, 200, 404, 500, 503]

    with OUTPUT.open("w") as f:
        for i in range(count):
            timestamp = start + timedelta(milliseconds=i * 10)
            ip = f"192.168.1.{random.randint(1, 254)}"
            method = random.choice(["GET", "POST"])
            path = random.choice(paths)
            status = random.choice(statuses)
            duration = random.randint(5, 1500)

            f.write(
                f"{timestamp.isoformat()} {ip} "
                f"{method} {path} {status} {duration}ms\n"
            )

    print(f"Generated {count:,} logs at {OUTPUT}")


if __name__ == "__main__":
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 100_000
    generate_logs(count)

