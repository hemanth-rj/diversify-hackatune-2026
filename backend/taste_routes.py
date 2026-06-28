from fastapi import APIRouter, HTTPException
from pydantic import BaseModel
import numpy as np
import data_loader, rerank, db_search
import learned_weights as _lw
import audio_features as _af

router = APIRouter()


class SteerOptions(BaseModel):
    pinIds: list[str] = []
    excludeMoods: list[str] = []


class TasteRequest(BaseModel):
    userId: str
    seedCount: int = 10
    limit: int = 20
    steer: SteerOptions | None = None


def _track_meta(cid: str) -> dict:
    meta = data_loader.TRACKS.get(cid, {})
    jid = meta.get("jamendo_id", "")
    return {
        "id": cid,
        "jamendoId": jid,
        "name": meta.get("name") or f"Track {jid}",
        "artist": meta.get("artist", "—"),
        "audioUrl": data_loader.audio_url(cid),
    }


@router.post("/taste")
def taste(req: TasteRequest):
    if req.userId not in data_loader.USERS:
        raise HTTPException(status_code=400, detail=f"User {req.userId} not found")

    seeds = data_loader.sample_seeds(req.userId, req.seedCount)
    if not seeds:
        raise HTTPException(status_code=400, detail="User has no resolvable liked tracks")

    # Prepend pinned IDs, deduplicate, cap at 10
    if req.steer and req.steer.pinIds:
        all_seeds = list(dict.fromkeys(req.steer.pinIds + seeds))[:10]
    else:
        all_seeds = seeds[:10]

    nin_models: dict[str, set] = {}
    if req.steer and req.steer.excludeMoods:
        nin_models["MoodSimpleV2"] = set(req.steer.excludeMoods)

    liked_set = set(data_loader.USERS.get(req.userId, []))

    # Load seed tag vectors directly from DB — no Cyanite API calls
    seed_tv_map = db_search.load_tag_vectors(all_seeds)
    seed_vectors = [tv for tv in seed_tv_map.values() if tv is not None]
    if not seed_vectors:
        return {"tracks": [], "profileSummary": "Could not load taste profile",
                "profileFingerprint": [], "seedTracks": [_track_meta(c) for c in all_seeds]}

    seed_mean = np.mean(np.stack(seed_vectors), axis=0)

    # Personalize global learned weights by this user's seed mean
    if _lw.LEARNED_WEIGHTS is not None:
        personal_w = seed_mean * _lw.LEARNED_WEIGHTS
        personal_w /= (personal_w.sum() + 1e-9)
        weights = personal_w
    else:
        profile = rerank.build_taste_profile(seed_vectors)
        weights = profile["weights"]

    # Score ALL local catalog tracks against personal weights — pure numpy, no API
    exclude = liked_set | set(all_seeds)
    tracks = db_search.score_catalog(weights, limit=req.limit,
                                     exclude_ids=exclude, nin_models=nin_models or None)

    # Blend in raw audio similarity (mel+chroma+mfcc+tonnetz, 382-dim)
    seed_audio = _af.get_embeddings(all_seeds)
    if seed_audio:
        seed_audio_vecs = np.stack(list(seed_audio.values()))
        seed_audio_mean = seed_audio_vecs.mean(axis=0)
        norm = np.linalg.norm(seed_audio_mean)
        if norm > 1e-9:
            seed_audio_mean /= norm
            result_audio = _af.get_embeddings([t["id"] for t in tracks])
            for track in tracks:
                rv = result_audio.get(track["id"])
                if rv is not None:
                    rv_norm = np.linalg.norm(rv)
                    if rv_norm > 1e-9:
                        audio_sim = float(np.dot(seed_audio_mean, rv / rv_norm))
                        track["finalScore"] = round(
                            0.6 * track["tagSim"] + 0.4 * audio_sim, 3
                        )
            tracks.sort(key=lambda t: t["finalScore"], reverse=True)

    # Top-5 dimensions by weight → "Your sound" fingerprint
    top_idx = np.argsort(-weights)[:5]
    fingerprint = [
        {"dimension": rerank.DIM_LABELS[i], "weight": round(float(weights[i]), 3)}
        for i in top_idx
    ]
    dim_names = [f["dimension"].split(".")[-1] for f in fingerprint]
    summary = f"Your sound: {' · '.join(dim_names)}"

    return {
        "tracks": tracks,
        "profileSummary": summary,
        "profileFingerprint": fingerprint,
        "seedTracks": [_track_meta(cid) for cid in all_seeds],
    }
