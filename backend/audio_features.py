"""
Audio feature queries against the pgvector tables.

Tables: vec_mel(288), vec_chroma(48), vec_mfcc(40), vec_tonnetz(6)
All have HNSW indexes with vector_cosine_ops.

Two public functions:
  get_embeddings(cids)       -> dict[cid, np.ndarray(382)]  batch lookup
  find_similar_by_id(cid, k) -> [(cid, cosine_sim), ...]    KNN via pgvector
"""
import os
import numpy as np
import psycopg2

_DB_URL = os.environ.get("DATABASE_URL", "")

_TABLES = [
    ("vec_mel",     288),
    ("vec_chroma",   48),
    ("vec_mfcc",     40),
    ("vec_tonnetz",   6),
]


def _connect():
    return psycopg2.connect(_DB_URL)


def _parse(vec_str: str) -> np.ndarray:
    """Parse pgvector string '[0.1,0.2,...]' → float32 ndarray."""
    return np.fromstring(vec_str.strip("[]"), sep=",", dtype=np.float32)


def get_embeddings(cids: list[str]) -> dict[str, np.ndarray]:
    """
    Fetch and concatenate 382-dim audio embeddings for the given cyanite_ids.
    Returns only tracks that have all 4 feature vectors present.
    """
    if not cids or not _DB_URL:
        return {}
    try:
        placeholders = ",".join(["%s"] * len(cids))
        parts: dict[str, list[np.ndarray]] = {cid: [] for cid in cids}
        with _connect() as conn:
            cur = conn.cursor()
            for table, _ in _TABLES:
                cur.execute(
                    f"SELECT cyanite_id, embedding FROM {table} WHERE cyanite_id IN ({placeholders})",
                    tuple(cids),
                )
                for cid, emb_str in cur.fetchall():
                    if cid in parts:
                        parts[cid].append(_parse(emb_str))
        return {
            cid: np.concatenate(vecs)
            for cid, vecs in parts.items()
            if len(vecs) == 4
        }
    except Exception as e:
        print(f"[audio_features] get_embeddings failed: {e}")
        return {}


def rerank_by_coherence(tracks: list[dict], alpha: float = 0.3) -> list[dict]:
    """
    Acoustic coherence pass for query-based results (Chat, MoodBoard).

    The top-5 Cyanite results define a "sonic neighbourhood" — the intended
    sound for this query. Re-rank the full result list by proximity to that
    neighbourhood's audio centroid.

    final = (1 - alpha) * current_finalScore + alpha * audio_coherence_sim

    Tracks without audio embeddings keep their current score unchanged.
    """
    if not tracks:
        return tracks

    cids = [t["id"] for t in tracks]
    embeddings = get_embeddings(cids)
    if not embeddings:
        return tracks

    # Centroid from top-5 by current score (Cyanite's semantic ranking)
    top5_vecs = [embeddings[t["id"]] for t in tracks[:5] if t["id"] in embeddings]
    if not top5_vecs:
        return tracks

    centroid = np.mean(np.stack(top5_vecs), axis=0)
    norm = np.linalg.norm(centroid)
    if norm < 1e-9:
        return tracks
    centroid /= norm

    for track in tracks:
        rv = embeddings.get(track["id"])
        if rv is None:
            continue
        rv_norm = np.linalg.norm(rv)
        if rv_norm < 1e-9:
            continue
        coherence = float(np.dot(centroid, rv / rv_norm))
        track["finalScore"] = round(
            (1 - alpha) * track["finalScore"] + alpha * coherence, 3
        )

    tracks.sort(key=lambda t: t["finalScore"], reverse=True)
    return tracks


def find_similar_by_id(
    cyanite_id: str,
    k: int = 50,
    exclude_ids: set | None = None,
) -> list[tuple[str, float]]:
    """
    Find k acoustically similar tracks using mel cosine similarity (HNSW).
    Returns [(cyanite_id, cosine_sim), ...] sorted descending.

    Uses mel alone for KNN (indexed, 288-dim is most discriminative).
    """
    if not _DB_URL:
        return []
    exclude = exclude_ids or set()
    try:
        with _connect() as conn:
            cur = conn.cursor()
            # Sub-select so we never parse and re-serialise the seed vector in Python
            cur.execute(
                """
                SELECT cyanite_id,
                       1 - (embedding <=> (SELECT embedding FROM vec_mel WHERE cyanite_id = %s)) AS score
                FROM vec_mel
                WHERE cyanite_id != %s
                ORDER BY embedding <=> (SELECT embedding FROM vec_mel WHERE cyanite_id = %s)
                LIMIT %s
                """,
                (cyanite_id, cyanite_id, cyanite_id, k + len(exclude) + 10),
            )
            rows = cur.fetchall()
        return [
            (cid, float(score))
            for cid, score in rows
            if cid not in exclude
        ][:k]
    except Exception as e:
        print(f"[audio_features] find_similar_by_id failed: {e}")
        return []
