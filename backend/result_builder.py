"""
Shared pipeline: Cyanite search items → enriched TrackResult dicts.

Call enrich_results() from every router. It handles:
  1. Extract cyanite_ids and scores from raw search items
  2. Batch-fetch missing tags from cache
  3. Build tag vectors
  4. Re-rank with TF-IDF weighted cosine
  5. Attach explanation, audio URL, valence/arousal
"""
import data_loader, cache, rerank, llm, idf
import numpy as np


def _track_result(cid: str, tags: dict | None, c_score: float,
                  tag_sim: float, final_score: float,
                  match_breakdown: list[dict]) -> dict:
    meta = data_loader.TRACKS.get(cid, {})
    auto_desc = ""
    rep_start = 0.0
    valence = 0.0
    arousal = 0.0
    if tags:
        ad = tags.get("AutoDescriptionV2", {})
        auto_desc = ad.get("description", "") or ""
        rs = tags.get("RepresentativeSegmentV2", {})
        rep_start = float(rs.get("startSeconds", 0) or 0)
        va_scores = tags.get("ValenceArousalV2", {}).get("scores", {})
        valence = (float(va_scores.get("valence", 0) or 0) + 1.0) / 2.0
        arousal = (float(va_scores.get("arousal", 0) or 0) + 1.0) / 2.0
    name = meta.get("name") or f"Track {meta.get('jamendo_id', cid)}"
    artist = meta.get("artist", "—")
    explanation = llm.explain_track(name, artist, match_breakdown, auto_desc)
    return {
        "id": cid,
        "jamendoId": meta.get("jamendo_id", ""),
        "name": name,
        "artist": artist,
        "duration": meta.get("duration", 0),
        "audioUrl": data_loader.audio_url(cid),
        "cyaniteScore": round(c_score, 3),
        "tagSim": tag_sim,
        "finalScore": final_score,
        "explanation": explanation,
        "matchBreakdown": match_breakdown,
        "autoDescription": auto_desc,
        "representativeSegmentStart": rep_start,
        "valence": round(valence, 3),
        "arousal": round(arousal, 3),
    }


def _register_from_title(cid: str, item: dict) -> bool:
    """If track not in DB, try to extract jamendo_id from Cyanite title field ('{jid}.mp3')."""
    if cid in data_loader.TRACKS:
        return True
    title = item.get("track", {}).get("title", "") or ""
    if not title.endswith(".mp3"):
        return False
    jid = title[:-4]
    data_loader.TRACKS[cid] = {"jamendo_id": jid, "name": "", "artist": "—", "duration": 0}
    data_loader.CYANITE_TO_JAMENDO[cid] = jid
    data_loader.JAMENDO_TO_CYANITE[jid] = cid
    return True


def enrich_results(
    raw_items: list[dict],
    seed_vectors: list[np.ndarray] | None = None,
    weights: np.ndarray | None = None,
    limit: int = 20,
) -> list[dict]:
    """
    raw_items: [{track: {id, ...}, score: float}] from Cyanite search
    seed_vectors: list of np.ndarray(86,) for seed tracks. When None or empty,
                  falls back to rerank_global (score = dot(tv, weights)).
    weights: re-ranking weight vector. None = uniform cosine (single-seed similar).
    """
    cids = [item["track"]["id"] for item in raw_items]
    c_scores = [float(item.get("score", 0)) for item in raw_items]

    # Resolve tracks: prefer DB metadata; fall back to title-derived jamendo_id
    resolvable = [(cid, score) for cid, score, item in zip(cids, c_scores, raw_items)
                  if _register_from_title(cid, item)]
    if not resolvable:
        return []
    cids, c_scores = zip(*resolvable)
    cids, c_scores = list(cids), list(c_scores)

    cache.batch_fetch_missing(cids)

    # Build per-result dicts with tag_vector attached
    enriched = []
    for cid, c_score in zip(cids, c_scores):
        tags = cache.get_tags(cid)
        tv = rerank.build_tag_vector(tags) if tags else None
        enriched.append({"_cid": cid, "_tags": tags, "tag_vector": tv, "_c_score": c_score})

    if seed_vectors:
        ranked = rerank.rerank(
            seed_vectors=seed_vectors,
            results=enriched,
            cyanite_scores=c_scores,
            weights=weights,
        )
    elif weights is not None:
        ranked = rerank.rerank_global(
            results=enriched,
            cyanite_scores=c_scores,
            weights=weights,
        )
    else:
        # No seeds, no weights — preserve Cyanite order
        for r, s in zip(enriched, c_scores):
            r["tag_sim"] = 0.0
            r["final_score"] = round(s, 3)
            r["match_breakdown"] = []
        ranked = enriched

    return [
        _track_result(
            cid=r["_cid"],
            tags=r["_tags"],
            c_score=r["_c_score"],
            tag_sim=r.get("tag_sim", 0.0),
            final_score=r.get("final_score", 0.0),
            match_breakdown=r.get("match_breakdown", []),
        )
        for r in ranked[:limit]
    ]
