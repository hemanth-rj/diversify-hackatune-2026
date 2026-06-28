"""
Global IDF weights for the 86-dim tag vector.

Computed once from a sample of 20 users × 5 tracks (~100 tag calls max).
Prefers tracks already in the in-process cache to avoid spending quota.
Saved to ../data/idf_cache.json. Loaded on subsequent starts.

IDF formula: log(N / (df + 1)) + 1   (smoothed, always > 0)
where N = number of sampled users, df = users for whom that dimension > 0.1

Rate limit: Cyanite tagging is 180/min shared across all teams (event quota 50k).
We sleep 0.35s between fresh fetches to stay comfortably under that cap.
"""
import json
import os
import random
import time

import numpy as np

_DATA = os.path.join(os.path.dirname(__file__), "..", "data")
_CACHE_PATH = os.path.join(_DATA, "idf_cache.json")

IDF_WEIGHTS: np.ndarray = np.ones(86, dtype=np.float32)  # default: uniform


def _compute(n_users: int = 20, tracks_per_user: int = 5) -> np.ndarray:
    import data_loader
    import cache
    import cyanite
    import rerank

    print(f"[idf] computing IDF from {n_users} users × {tracks_per_user} tracks ...")
    uids = random.sample(
        list(data_loader.USERS.keys()),
        min(n_users, len(data_loader.USERS)),
    )
    doc_freq = np.zeros(86, dtype=np.float32)
    n_docs = 0

    for uid in uids:
        cids = data_loader.sample_seeds(uid, tracks_per_user)
        user_vecs = []
        for cid in cids:
            tags = cache.get_tags(cid)
            if tags is None:
                # Only fetch if not already cached; respect shared rate limit
                try:
                    time.sleep(0.35)  # 180/min = 3/s; 0.35s gives comfortable headroom
                    tags = cyanite.get_model_outputs(cid)
                    cache.store_tags(cid, tags)
                except Exception as e:
                    print(f"[idf] skipping {cid}: {e}")
                    continue
            user_vecs.append(rerank.build_tag_vector(tags))
        if not user_vecs:
            continue
        mean_vec = np.stack(user_vecs).mean(axis=0)
        doc_freq += (mean_vec > 0.1).astype(np.float32)
        n_docs += 1

    if n_docs == 0:
        return np.ones(86, dtype=np.float32)

    idf = np.log(n_docs / (doc_freq + 1.0)) + 1.0
    idf = idf / (idf.sum() + 1e-9) * 86  # scale so mean weight ≈ 1
    return idf.astype(np.float32)


def _compute_and_save() -> None:
    """Run in a background thread when no cache file exists."""
    global IDF_WEIGHTS
    try:
        weights = _compute()
        IDF_WEIGHTS = weights
        os.makedirs(_DATA, exist_ok=True)
        with open(_CACHE_PATH, "w") as f:
            json.dump(weights.tolist(), f)
        print(f"[idf] saved to {_CACHE_PATH}")
    except Exception as e:
        print(f"[idf] background compute failed: {e}")


def ensure_loaded() -> None:
    global IDF_WEIGHTS
    if os.path.exists(_CACHE_PATH):
        with open(_CACHE_PATH) as f:
            IDF_WEIGHTS = np.array(json.load(f), dtype=np.float32)
        print(f"[idf] loaded from {_CACHE_PATH}")
        return
    # No cache — start background computation so startup isn't blocked.
    # IDF_WEIGHTS stays at uniform (all-ones) until the thread finishes.
    import threading
    print("[idf] no cache found — computing in background (uniform weights until done)")
    threading.Thread(target=_compute_and_save, daemon=True).start()
