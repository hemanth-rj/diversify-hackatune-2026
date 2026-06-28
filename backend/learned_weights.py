"""
Global re-ranking weights learned from leave-one-out validation across all users.
Loaded from data/learned_weights.json (produced by compute_learned_weights.py).
Falls back to None if the file doesn't exist yet.
"""
import json
import os
import numpy as np

_PATH = os.path.join(os.path.dirname(__file__), "..", "data", "learned_weights.json")

LEARNED_WEIGHTS: np.ndarray | None = None
N_FOLDS: int = 0
N_USERS: int = 0


def ensure_loaded() -> None:
    global LEARNED_WEIGHTS, N_FOLDS, N_USERS
    if LEARNED_WEIGHTS is not None:
        return
    if not os.path.exists(_PATH):
        print("[learned_weights] learned_weights.json not found — re-ranking will use IDF fallback")
        return
    with open(_PATH) as f:
        data = json.load(f)
    LEARNED_WEIGHTS = np.array(data["weights"], dtype=np.float32)
    N_FOLDS = data.get("n_folds", 0)
    N_USERS = data.get("n_users", 0)
    print(f"[learned_weights] loaded — {N_FOLDS} LOO folds across {N_USERS} users")
