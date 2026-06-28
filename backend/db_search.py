"""
DB-backed search pipeline — no Cyanite API calls.
Reads cyanite_tag + jamendo_track; scores with numpy against learned weights.
"""
import os, threading, psycopg2, numpy as np
from collections import defaultdict
import data_loader, rerank

_DB_URL = os.environ.get("DATABASE_URL", "")

# Lazy in-memory caches (tag vectors + valence/arousal), loaded once on first use
_ALL_VECTORS: dict[str, np.ndarray] | None = None
_ALL_VA: dict[str, tuple] | None = None
_LOCK = threading.Lock()


def _quick_explain(match_breakdown: list[dict]) -> str:
    if not match_breakdown:
        return "Acoustically similar to your search."
    labels = [d["dimension"].split(".")[-1] for d in match_breakdown[:2]]
    return f"Shares {' · '.join(labels)} characteristics."


def _conn():
    return psycopg2.connect(_DB_URL)


def _rows_to_tag_outputs(rows: list) -> dict:
    """(model, tag, score) rows → tag_outputs dict for rerank.build_tag_vector."""
    by_model: dict[str, dict] = defaultdict(dict)
    for model, tag, score in rows:
        by_model[model][tag] = score
    out = {}
    for model, td in by_model.items():
        if model == "InstrumentsV2":
            pres = {t: "throughout" for t, s in td.items() if s is None}
            if pres:
                out["InstrumentsV2"] = {"presence": pres}
        else:
            out[model] = {"scores": {t: float(s) if s is not None else 0.5 for t, s in td.items()}}
    return out


def load_tag_vectors(cids: list[str]) -> dict[str, np.ndarray]:
    """Fetch tag vectors for specific IDs from DB (one query)."""
    if not cids or not _DB_URL:
        return {}
    try:
        with _conn() as conn:
            cur = conn.cursor()
            ph = ",".join(["%s"] * len(cids))
            cur.execute(
                f"SELECT cyanite_id, model, tag, score FROM cyanite_tag WHERE cyanite_id IN ({ph})",
                cids,
            )
            rows = cur.fetchall()
    except Exception as e:
        print(f"[db_search] load_tag_vectors error: {e}")
        return {}
    track_rows: dict[str, list] = defaultdict(list)
    for cid, model, tag, score in rows:
        track_rows[cid].append((model, tag, score))
    return {
        cid: rerank.build_tag_vector(_rows_to_tag_outputs(r))
        for cid, r in track_rows.items()
    }


def all_tag_vectors() -> dict[str, np.ndarray]:
    """Lazy-loaded in-memory cache: all local tracks with Cyanite analysis."""
    global _ALL_VECTORS
    if _ALL_VECTORS is not None:
        return _ALL_VECTORS
    with _LOCK:
        if _ALL_VECTORS is not None:
            return _ALL_VECTORS
        _ALL_VECTORS = _load_all()
        return _ALL_VECTORS


def all_va() -> dict[str, tuple]:
    """Lazy-loaded (valence, arousal) normalized to 0-1 for all tracks in cyanite_track."""
    global _ALL_VA
    if _ALL_VA is not None:
        return _ALL_VA
    with _LOCK:
        if _ALL_VA is not None:
            return _ALL_VA
        if not _DB_URL:
            _ALL_VA = {}
            return _ALL_VA
        try:
            with _conn() as conn:
                cur = conn.cursor()
                cur.execute("SELECT cyanite_id, valence, arousal FROM cyanite_track")
                rows = cur.fetchall()
        except Exception as e:
            print(f"[db_search] all_va error: {e}")
            _ALL_VA = {}
            return _ALL_VA
        _ALL_VA = {
            cid: (
                round((float(v) + 1.0) / 2.0, 3) if v is not None else None,
                round((float(a) + 1.0) / 2.0, 3) if a is not None else None,
            )
            for cid, v, a in rows
        }
        return _ALL_VA


def _load_all() -> dict[str, np.ndarray]:
    if not _DB_URL:
        return {}
    try:
        with _conn() as conn:
            cur = conn.cursor()
            cur.execute("SELECT cyanite_id, model, tag, score FROM cyanite_tag")
            rows = cur.fetchall()
    except Exception as e:
        print(f"[db_search] _load_all error: {e}")
        return {}
    track_rows: dict[str, list] = defaultdict(list)
    for cid, model, tag, score in rows:
        track_rows[cid].append((model, tag, score))
    result = {}
    for cid, r in track_rows.items():
        meta = data_loader.TRACKS.get(cid, {})
        if not meta.get("name"):
            continue
        result[cid] = rerank.build_tag_vector(_rows_to_tag_outputs(r))
    print(f"[db_search] cached {len(result)} tag vectors from DB")
    return result


# ── scoring ───────────────────────────────────────────────────────────────────

def _make_result(cid: str, tv, weights: np.ndarray, c_score: float = 0.5,
                 va: tuple = (None, None)) -> dict | None:
    meta = data_loader.TRACKS.get(cid, {})
    if not meta.get("name"):
        return None
    jid = meta.get("jamendo_id", "")
    name = meta.get("name") or f"Track {jid}"
    artist = meta.get("artist", "—")
    tag_sim, final_score, match_breakdown = 0.0, round(c_score, 3), []
    if tv is not None:
        tag_score = float(np.dot(tv, weights))
        final_score = round(0.5 * tag_score + 0.5 * c_score, 3)
        tag_sim = round(tag_score, 3)
        top_w = list(np.argsort(-weights)[:20])
        cands = [(j, float(tv[j])) for j in top_w if float(tv[j]) > 0.05]
        cands.sort(key=lambda x: -x[1])
        match_breakdown = [
            {
                "dimension": rerank.DIM_LABELS[j],
                "seedScore": round(float(tv[j]), 3),
                "resultScore": round(float(tv[j]), 3),
                "delta": 0.0,
            }
            for j, _ in cands[:3]
        ]
    valence, arousal = va
    return {
        "id": cid,
        "jamendoId": jid,
        "name": name,
        "artist": artist,
        "duration": meta.get("duration", 0),
        "audioUrl": data_loader.audio_url(cid),
        "cyaniteScore": round(c_score, 3),
        "tagSim": tag_sim,
        "finalScore": final_score,
        "explanation": _quick_explain(match_breakdown),
        "matchBreakdown": match_breakdown,
        "autoDescription": "",
        "representativeSegmentStart": 0,
        "valence": valence,
        "arousal": arousal,
    }


def _is_excluded_by_nin(tv: np.ndarray, nin_models: dict[str, set]) -> bool:
    """Return True if track's tag vector matches any $nin exclusion above threshold."""
    for model, nin_tags in nin_models.items():
        if model == "MoodSimpleV2":
            for i, mood in enumerate(rerank.MOOD_KEYS):
                if mood in nin_tags and float(tv[i]) > 0.4:
                    return True
    return False


# ── public API ────────────────────────────────────────────────────────────────

def search_by_filter(
    meta_filter: dict,
    weights: np.ndarray,
    limit: int = 10,
    exclude_ids: set | None = None,
    strict: bool = True,
) -> list[dict]:
    """
    Query cyanite_tag by LLM meta_filter → load tag vectors → score → return TrackResults.
    strict=True: track must match every model (AND across models).
    strict=False: track must match any one model (OR across models, broader fallback).
    """
    if not meta_filter or not _DB_URL:
        return []

    # Separate $in per model and $nin per model
    model_in: dict[str, list] = {}
    nin_models: dict[str, set] = {}
    for key, cond in meta_filter.items():
        if not key.endswith(".tags"):
            continue
        model = key.replace(".tags", "")
        for op, tags in cond.items():
            if not tags:
                continue
            if op == "$in":
                model_in.setdefault(model, []).extend(tags)
            elif op == "$nin":
                nin_models[model] = set(tags)

    if not model_in:
        return []

    # WHERE: track has at least one matching tag from any model
    # HAVING: track must match EACH model (AND across models, OR within a model)
    where_parts, where_params, having_parts, having_params = [], [], [], []
    for model, tags in model_in.items():
        ph = ",".join(["%s"] * len(tags))
        where_parts.append(f"(model = %s AND tag IN ({ph}))")
        where_params.extend([model] + tags)
        having_parts.append(
            f"SUM(CASE WHEN model = %s AND tag IN ({ph}) THEN 1 ELSE 0 END) > 0"
        )
        having_params.extend([model] + tags)

    where_clause = " OR ".join(where_parts)
    having_clause = " AND ".join(having_parts)

    try:
        with _conn() as conn:
            cur = conn.cursor()
            if strict:
                cur.execute(
                    f"""SELECT cyanite_id, AVG(COALESCE(score, 0.5)) AS ms
                        FROM cyanite_tag
                        WHERE {where_clause}
                        GROUP BY cyanite_id
                        HAVING {having_clause}
                        ORDER BY ms DESC LIMIT %s""",
                    where_params + having_params + [limit * 5],
                )
            else:
                cur.execute(
                    f"""SELECT cyanite_id, AVG(COALESCE(score, 0.5)) AS ms
                        FROM cyanite_tag
                        WHERE {where_clause}
                        GROUP BY cyanite_id
                        ORDER BY ms DESC LIMIT %s""",
                    where_params + [limit * 5],
                )
            candidates = cur.fetchall()
    except Exception as e:
        print(f"[db_search] search_by_filter error: {e}")
        return []

    cids = [cid for cid, _ in candidates if not exclude_ids or cid not in exclude_ids]
    c_map = {cid: float(s) for cid, s in candidates}
    tv_map = load_tag_vectors(cids)
    va_map = all_va()

    results = []
    for cid in cids:
        tv = tv_map.get(cid)
        if tv is not None and nin_models and _is_excluded_by_nin(tv, nin_models):
            continue
        r = _make_result(cid, tv, weights, c_map.get(cid, 0.5), va=va_map.get(cid, (None, None)))
        if r:
            results.append(r)

    results.sort(key=lambda r: r["finalScore"], reverse=True)
    return results[:limit]


def score_catalog(
    weights: np.ndarray,
    limit: int = 20,
    exclude_ids: set | None = None,
    nin_models: dict | None = None,
) -> list[dict]:
    """
    Score ALL locally cached tag vectors against weights.
    Used for taste recommendations — no API calls, pure numpy.
    """
    tvs = all_tag_vectors()
    va_map = all_va()
    results = []
    for cid, tv in tvs.items():
        if exclude_ids and cid in exclude_ids:
            continue
        if nin_models and _is_excluded_by_nin(tv, nin_models):
            continue
        r = _make_result(cid, tv, weights, va=va_map.get(cid, (None, None)))
        if r:
            results.append(r)
    results.sort(key=lambda r: r["finalScore"], reverse=True)
    return results[:limit]


def _prewarm() -> None:
    try:
        all_tag_vectors()
        all_va()
        print("[db_search] prewarm complete")
    except Exception as e:
        print(f"[db_search] prewarm error: {e}")


threading.Thread(target=_prewarm, daemon=True).start()
