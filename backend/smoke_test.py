#!/usr/bin/env python3
"""End-to-end smoke test — requires the server to be running at http://localhost:8001."""
import sys

try:
    import requests
except ImportError:
    print("ERROR: 'requests' package not installed. Run: pip install requests")
    sys.exit(1)

BASE = "http://localhost:8001"

# Real IDs from data/
USER_ID = "545127"
SEED_CYANITE_ID = "libtr_01KVX1J122H6RS7K1F"


def _check_server():
    try:
        r = requests.get(f"{BASE}/api/health", timeout=5)
        r.raise_for_status()
    except requests.exceptions.ConnectionError:
        print(f"ERROR: Cannot connect to server at {BASE}.")
        print("Start the server first with: uvicorn app:app --port 8001")
        sys.exit(1)
    except Exception as e:
        print(f"ERROR: Health check failed: {e}")
        sys.exit(1)


def _run(label: str, r: requests.Response, *, extra: str = "") -> bool:
    if r.status_code == 200:
        d = r.json()
        tracks = d.get("tracks", [])
        n = len(tracks)
        first = tracks[0].get("name", "?") if n else "—"
        suffix = f", {extra}" if extra else ""
        print(f"→ [{label:<12}] OK  ({n} results{suffix}, first: {first!r})")
        return True
    else:
        print(f"→ [{label:<12}] FAIL  HTTP {r.status_code}: {r.text[:300]}")
        return False


def main():
    _check_server()

    results = []

    results.append(_run(
        "chat",
        requests.post(f"{BASE}/api/chat", json={
            "messages": [{"role": "user", "content": "dark ambient piano, slow and emotional"}],
            "limit": 5,
        }),
    ))

    results.append(_run(
        "taste",
        requests.post(f"{BASE}/api/taste", json={"userId": USER_ID, "limit": 5}),
        extra=f"user {USER_ID}",
    ))

    results.append(_run(
        "similar",
        requests.post(f"{BASE}/api/similar", json={"seedId": SEED_CYANITE_ID, "limit": 5}),
        extra=f"seed {SEED_CYANITE_ID}",
    ))

    results.append(_run(
        "multimodal",
        requests.post(f"{BASE}/api/multimodal", json={
            "brief": "Tense night chase through a city, fast and suspenseful",
            "limit": 5,
        }),
        extra='brief "tense night chase"',
    ))

    failed = results.count(False)
    if failed:
        print(f"\n{failed}/{len(results)} checks FAILED")
        sys.exit(1)
    else:
        print(f"\nAll {len(results)} checks passed.")


if __name__ == "__main__":
    main()
