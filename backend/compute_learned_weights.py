#!/usr/bin/env python3
"""
Compute global re-ranking weights via leave-one-out validation across all users.

For every (user, liked_track) fold:
  seed_mean = mean tag vector of that user's OTHER liked tracks
  agreement = seed_mean * tag_vector(held_out_track)   [element-wise]

A dimension gets high agreement only when it's simultaneously high in the
search seeds AND in the withheld liked track — meaning it reliably predicts
what the user will like beyond the tracks used for search.

Average agreement across all folds × IDF → learned_weights.json.

Usage (run from the discovery/ directory on the server):
  python compute_learned_weights.py

Rate limit: Cyanite tagging shared at 180 req/min. This script sleeps 0.35s
between fresh fetches. Tracks already seen within this run are cached in memory.
"""
import json
import os
import sys
import time

# Load .env so DATABASE_URL etc. are available
try:
    from dotenv import load_dotenv
    load_dotenv()
except Exception:
    pass

import numpy as np

# Make sure imports resolve from this directory
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import data_loader   # populates USERS, TRACKS
import cache         # in-memory tag store
import cyanite       # get_model_outputs
import rerank
import idf as _idf

_DATA = os.path.join(os.path.dirname(__file__), "..", "data")
_OUT = os.path.join(_DATA, "learned_weights.json")


def fetch_tags_ratelimited(cids: list[str]) -> None:
    """Fetch tags for any cids not already in the in-memory cache."""
    missing = [c for c in cids if cache.get_tags(c) is None]
    if not missing:
        return
    print(f"  fetching {len(missing)} tracks from Cyanite ...", flush=True)
    for i, cid in enumerate(missing):
        try:
            time.sleep(0.35)
            tags = cyanite.get_model_outputs(cid)
            cache.store_tags(cid, tags)
        except Exception as e:
            print(f"  [skip] {cid}: {e}")
        if (i + 1) % 20 == 0:
            print(f"  {i+1}/{len(missing)} fetched", flush=True)


def main() -> None:
    _idf.ensure_loaded()

    users = data_loader.USERS  # {uid: [cid, ...]}
    print(f"Users: {len(users)}, computing unique liked track IDs ...")

    # Collect all unique liked track IDs across all users
    all_liked_ids: set[str] = set()
    for cids in users.values():
        all_liked_ids.update(cids)
    print(f"Unique liked tracks: {len(all_liked_ids)}")

    # Fetch tags for all liked tracks (rate-limited)
    fetch_tags_ratelimited(list(all_liked_ids))

    # Leave-one-out across all users and all liked tracks
    all_agreements: list[np.ndarray] = []
    skipped_users = 0
    total_folds = 0

    for uid, liked_ids in users.items():
        # Build tag vectors only for tracks we have tags for
        id_to_vec: dict[str, np.ndarray] = {}
        for cid in liked_ids:
            tags = cache.get_tags(cid)
            if tags is not None:
                id_to_vec[cid] = rerank.build_tag_vector(tags)

        if len(id_to_vec) < 2:
            skipped_users += 1
            continue

        valid = list(id_to_vec.keys())
        vecs = np.stack([id_to_vec[c] for c in valid])  # (n, 86)

        # Leave-one-out: for each track, seed_mean = mean of all others
        n = len(valid)
        total_sum = vecs.sum(axis=0)

        for i in range(n):
            held_out_vec = vecs[i]
            seed_sum = total_sum - held_out_vec
            seed_mean = seed_sum / max(n - 1, 1)
            # Agreement: high only where BOTH seeds and held-out are high
            agreement = seed_mean * held_out_vec
            all_agreements.append(agreement)
            total_folds += 1

    print(f"\nTotal LOO folds: {total_folds}  (skipped {skipped_users} users with < 2 cached tracks)")

    if not all_agreements:
        print("ERROR: no agreement vectors — is the tag cache empty?")
        sys.exit(1)

    # Average agreement × IDF → normalize
    global_agreement = np.mean(np.stack(all_agreements), axis=0)
    idf_weights = _idf.IDF_WEIGHTS
    combined = global_agreement * idf_weights
    total = combined.sum()
    learned = (combined / (total + 1e-9)).astype(np.float32)

    os.makedirs(_DATA, exist_ok=True)
    with open(_OUT, "w") as f:
        json.dump({
            "weights": learned.tolist(),
            "n_folds": total_folds,
            "n_users": len(users) - skipped_users,
            "dim_labels": rerank.DIM_LABELS,
        }, f)

    print(f"\nSaved → {_OUT}")
    top5 = np.argsort(-learned)[:5]
    print("Top 5 learned dimensions:")
    for i in top5:
        print(f"  {rerank.DIM_LABELS[i]:<35} {learned[i]:.4f}")


if __name__ == "__main__":
    main()
