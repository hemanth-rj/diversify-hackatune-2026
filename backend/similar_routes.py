from fastapi import APIRouter, HTTPException, Query, UploadFile, File
from pydantic import BaseModel
import numpy as np
import data_loader, db_search
import audio_features as _af
import json, os, subprocess, tempfile, psycopg2

_DB_URL = os.environ.get("DATABASE_URL", "")

router = APIRouter()

_PREDICT_MODELS = ["MoodSimpleV2", "MainGenreV2", "InstrumentsV2", "VocalsV2", "CharacterV2"]


def _predict_tags_from_hits(hits: list[dict], per_model: int = 5) -> dict:
    """Beta-Bernoulli tag transfer from acoustic nearest neighbors.
    hits = [{cyanite_id, dist}] from Rust binary. Returns {model: [{tag, prob}]}.
    """
    cids = [h["cyanite_id"] for h in hits if h.get("cyanite_id")]
    if not cids or not _DB_URL:
        return {}
    w = {h["cyanite_id"]: 1.0 / (1.0 + max(0.0, float(h.get("dist", 0.0))))
         for h in hits if h.get("cyanite_id")}
    totw = sum(w.values()) + 1e-12
    totw2 = sum(v * v for v in w.values()) + 1e-12
    neff = totw * totw / totw2  # Kish effective sample size
    try:
        conn = psycopg2.connect(_DB_URL)
        cur = conn.cursor()
        ph = ",".join(["%s"] * len(cids))
        cur.execute(
            f"SELECT cyanite_id, model, tag, score FROM cyanite_tag "
            f"WHERE cyanite_id IN ({ph}) AND model = ANY(%s)",
            cids + [_PREDICT_MODELS],
        )
        rows = cur.fetchall()
        conn.close()
    except Exception as e:
        print(f"[predict_tags] DB error: {e}")
        return {}

    # Aggregate weighted positive evidence per (model, tag)
    agg: dict[tuple, list] = {}
    for cid, model, tag, score in rows:
        wi = w.get(cid, 0.0)
        if wi <= 0:
            continue
        key = (model, tag)
        if key not in agg:
            agg[key] = [0.0, 0]
        agg[key][0] += wi
        agg[key][1] += 1

    try:
        from scipy.stats import beta as _beta
        use_bayes = True
    except ImportError:
        use_bayes = False

    bymodel: dict[str, list] = {}
    for (model, tag), (posw, nsup) in agg.items():
        pos_eff = neff * min(1.0, posw / totw)
        if use_bayes:
            # Beta-Bernoulli: flat prior (tau=2), posterior mean
            a_post = 1.0 + pos_eff
            b_post = 1.0 + max(0.0, neff - pos_eff)
            prob = round(a_post / (a_post + b_post), 3)
        else:
            prob = round(posw / totw, 3)
        bymodel.setdefault(model, []).append({"tag": tag, "prob": prob, "n": nsup})

    return {
        model: sorted(chips, key=lambda c: -c["prob"])[:per_model]
        for model, chips in bymodel.items()
    }


def _db():
    return psycopg2.connect(_DB_URL)


def _parse_vec(s: str) -> list[float]:
    return [float(x) for x in s.strip("[]").split(",")]


@router.get("/track/{cid}")
def track_detail(cid: str):
    meta = data_loader.TRACKS.get(cid)
    if not meta:
        raise HTTPException(status_code=404, detail="Track not found")
    jid = meta.get("jamendo_id", "")

    result: dict = {
        "id": cid,
        "jamendoId": jid,
        "name": meta.get("name") or f"Track {jid}",
        "artist": meta.get("artist", "—"),
        "duration": meta.get("duration", 0),
        "audioUrl": data_loader.audio_url(cid),
        # filled from DB below
        "key": None, "bpm": None, "timeSignature": None,
        "valence": None, "arousal": None, "energyLevel": None,
        "vocalPresence": None, "vocalGender": None, "description": None,
        "license": None,
        "tags": {},
        "rhythm": None, "pitch": None, "spectral": None,
        "spectralAubio": None, "rms": None, "zcr": None, "notes": None,
        "chromaVec": None, "mfccVec": None,
    }

    if not _DB_URL:
        return result

    try:
        with _db() as conn:
            cur = conn.cursor()

            # cyanite_track — key, BPM, valence, arousal, description
            cur.execute(
                "SELECT bpm, key, time_signature, valence, arousal, energy_level, "
                "vocal_presence, vocal_gender, description "
                "FROM cyanite_track WHERE cyanite_id = %s", (cid,)
            )
            row = cur.fetchone()
            if row:
                result.update({
                    "bpm": row[0], "key": row[1], "timeSignature": row[2],
                    "valence": round(float(row[3]), 3) if row[3] is not None else None,
                    "arousal": round(float(row[4]), 3) if row[4] is not None else None,
                    "energyLevel": row[5], "vocalPresence": row[6],
                    "vocalGender": row[7], "description": row[8],
                })

            # jamendo_track — license
            if jid:
                cur.execute("SELECT license_ccurl FROM jamendo_track WHERE jamendo_id = %s", (str(jid),))
                row = cur.fetchone()
                if row:
                    result["license"] = row[0]

            # cyanite_tag — all models
            cur.execute(
                "SELECT model, tag, score FROM cyanite_tag WHERE cyanite_id = %s ORDER BY model, score DESC NULLS LAST",
                (cid,)
            )
            tags: dict[str, list] = {}
            for model, tag, score in cur.fetchall():
                tags.setdefault(model, []).append({
                    "tag": tag,
                    "score": round(float(score), 4) if score is not None else None,
                })
            result["tags"] = tags

            # rhythm
            cur.execute("SELECT tempo_bpm, beat_bpm, beat_count, beat_regularity, tempo_confidence, onset_rate FROM rhythm WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                cols = ["tempo_bpm", "beat_bpm", "beat_count", "beat_regularity", "tempo_confidence", "onset_rate"]
                result["rhythm"] = {c: (round(float(v), 3) if v is not None else None) for c, v in zip(cols, row)}

            # pitch
            cur.execute("SELECT median_hz, voiced_frac, voiced_conf, tuning FROM pitch WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                result["pitch"] = {c: (round(float(v), 3) if v is not None else None)
                                   for c, v in zip(["median_hz", "voiced_frac", "voiced_conf", "tuning"], row)}

            # spectral
            cur.execute("SELECT centroid_mean, centroid_std, centroid_median, centroid_mad, rolloff_mean, rolloff_std, rolloff_median, rolloff_mad, bandwidth_mean, bandwidth_std, bandwidth_median, bandwidth_mad, flatness_mean, flatness_std, flatness_median, flatness_mad, flux_mean, flux_std, flux_median, flux_mad FROM spectral WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                cols = ["centroid_mean","centroid_std","centroid_median","centroid_mad","rolloff_mean","rolloff_std","rolloff_median","rolloff_mad","bandwidth_mean","bandwidth_std","bandwidth_median","bandwidth_mad","flatness_mean","flatness_std","flatness_median","flatness_mad","flux_mean","flux_std","flux_median","flux_mad"]
                result["spectral"] = {c: (round(float(v), 3) if v is not None else None) for c, v in zip(cols, row)}

            # spectral_aubio
            cur.execute("SELECT skewness_mean, skewness_std, kurtosis_mean, kurtosis_std, slope_mean, slope_std, decrease_mean, decrease_std FROM spectral_aubio WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                cols = ["skewness_mean","skewness_std","kurtosis_mean","kurtosis_std","slope_mean","slope_std","decrease_mean","decrease_std"]
                result["spectralAubio"] = {c: (round(float(v), 3) if v is not None else None) for c, v in zip(cols, row)}

            # rms
            cur.execute("SELECT mean, std, median, mad FROM rms WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                result["rms"] = {c: (round(float(v), 3) if v is not None else None)
                                 for c, v in zip(["mean","std","median","mad"], row)}

            # zcr
            cur.execute("SELECT mean, std, median, mad FROM zcr WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                result["zcr"] = {c: (round(float(v), 3) if v is not None else None)
                                 for c, v in zip(["mean","std","median","mad"], row)}

            # notes
            cur.execute("SELECT note_count, note_mean_dur FROM notes WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                result["notes"] = {"note_count": row[0], "note_mean_dur": round(float(row[1]), 4) if row[1] is not None else None}

            # vec_chroma (48-dim) for pitch profile visualization
            cur.execute("SELECT embedding FROM vec_chroma WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                result["chromaVec"] = [round(v, 4) for v in _parse_vec(row[0])]

            # vec_mfcc (40-dim) for timbre visualization
            cur.execute("SELECT embedding FROM vec_mfcc WHERE cyanite_id = %s", (cid,))
            row = cur.fetchone()
            if row:
                result["mfccVec"] = [round(v, 4) for v in _parse_vec(row[0])]

    except Exception as e:
        print(f"[track_detail] DB error for {cid}: {e}")

    return result


@router.get("/tracks/search")
def search_tracks(q: str = Query(..., min_length=1), limit: int = 15):
    q_lower = q.lower().strip()
    matches = [
        {"id": cid, "name": meta["name"], "artist": meta["artist"], "jamendoId": meta["jamendo_id"]}
        for cid, meta in data_loader.TRACKS.items()
        if q_lower in (meta.get("name") or "").lower()
    ]
    matches.sort(key=lambda x: (x["name"].lower().index(q_lower), x["name"].lower()))
    return matches[:limit]


_QUERY_BIN = os.path.expanduser("~/mml-hackatune-26/harvester/target/release/query")


@router.post("/similar-file")
async def similar_file(file: UploadFile = File(...), limit: int = 20):
    suffix = os.path.splitext(file.filename or "upload")[1] or ".mp3"
    with tempfile.NamedTemporaryFile(suffix=suffix, delete=False) as tmp:
        tmp.write(await file.read())
        tmp_path = tmp.name
    try:
        result = subprocess.run(
            [_QUERY_BIN, "similar-file", tmp_path, "--k", str(limit + 5)],
            capture_output=True, text=True, timeout=30,
            env={**os.environ, "DATABASE_URL": _DB_URL},
        )
        if result.returncode != 0:
            raise HTTPException(status_code=500, detail=f"Feature extraction failed: {result.stderr[:200]}")
        hits = json.loads(result.stdout)
    finally:
        os.unlink(tmp_path)

    va_map = db_search.all_va()
    tracks = []
    for hit in hits[:limit]:
        cid = hit["cyanite_id"]
        meta = data_loader.TRACKS.get(cid, {})
        if not meta:
            continue
        jid = meta.get("jamendo_id", "")
        va = va_map.get(cid, (None, None))
        tracks.append({
            "id": cid,
            "jamendoId": jid,
            "name": meta.get("name") or f"Track {jid}",
            "artist": meta.get("artist", "—"),
            "duration": meta.get("duration", 0),
            "audioUrl": data_loader.audio_url(cid),
            "dist": round(hit["dist"], 4),
            "matchBreakdown": [],
            # Rust dist = weighted sum of 4 cosine distances (weights: mel=1, chroma=1, tonnetz=0.5, mfcc=1)
            # Max theoretical value is (1+1+0.5+1)*2 = 7.0; normalise to [0,1].
            "finalScore": round(max(0.0, 1.0 - hit["dist"] / 7.0), 3),
            "cyaniteScore": 0.0,
            "tagSim": 0.0,
            "explanation": "",
            "autoDescription": "",
            "representativeSegmentStart": 0.0,
            "valence": va[0],
            "arousal": va[1],
        })

    predicted_tags = _predict_tags_from_hits(hits)
    return {"tracks": tracks, "predicted_tags": predicted_tags}


@router.get("/tag/{model}/{tag}")
def tag_explore(model: str, tag: str, limit: int = 15):
    """Top tracks by Cyanite confidence score for a given model+tag."""
    if not _DB_URL:
        return {"model": model, "tag": tag, "tracks": []}
    try:
        with _db() as conn:
            cur = conn.cursor()
            cur.execute(
                """SELECT ct.cyanite_id, ct.score
                   FROM cyanite_tag ct
                   WHERE ct.model = %s AND ct.tag = %s
                   ORDER BY COALESCE(ct.score, RANDOM()) DESC
                   LIMIT %s""",
                (model, tag, limit),
            )
            rows = cur.fetchall()
    except Exception as e:
        print(f"[tag_explore] DB error: {e}")
        return {"model": model, "tag": tag, "tracks": []}

    tracks = []
    for cid, score in rows:
        m = data_loader.TRACKS.get(cid)
        if not m:
            continue
        jid = m.get("jamendo_id", "")
        tracks.append({
            "id": cid,
            "jamendoId": jid,
            "name": m.get("name") or f"Track {jid}",
            "artist": m.get("artist", "—"),
            "duration": m.get("duration", 0),
            "audioUrl": data_loader.audio_url(cid),
            "tagScore": round(float(score), 4) if score is not None else 1.0,
        })
    return {"model": model, "tag": tag, "tracks": tracks}


class SimilarRequest(BaseModel):
    seedId: str
    limit: int = 20


@router.post("/similar")
def similar(req: SimilarRequest):
    # Load seed tag vector from DB — no Cyanite API
    seed_tv_map = db_search.load_tag_vectors([req.seedId])
    seed_tv = seed_tv_map.get(req.seedId)
    if seed_tv is None:
        raise HTTPException(status_code=404,
                            detail=f"No tag data for seed {req.seedId}")

    # Use seed tag vector as per-dimension weights
    weights = seed_tv / (seed_tv.sum() + 1e-9)

    # Score full local catalog by tag similarity — pure numpy, no API
    tracks = db_search.score_catalog(
        weights, limit=req.limit * 2, exclude_ids={req.seedId}
    )

    # Blend in acoustic similarity from pgvector HNSW index
    audio_hits = _af.find_similar_by_id(req.seedId, k=req.limit * 2)
    if audio_hits:
        audio_map = {cid: score for cid, score in audio_hits}
        for t in tracks:
            a_sim = audio_map.get(t["id"], 0.5)
            t["finalScore"] = round(0.6 * t["tagSim"] + 0.4 * a_sim, 3)
        tracks.sort(key=lambda t: t["finalScore"], reverse=True)

    tracks = tracks[: req.limit]

    return {
        "seed": _seed_result(req.seedId),
        "tracks": tracks,
    }


def _seed_result(cid: str) -> dict:
    meta = data_loader.TRACKS.get(cid, {})
    va = db_search.all_va().get(cid, (None, None))
    return {
        "id": cid,
        "jamendoId": meta.get("jamendo_id", ""),
        "name": meta.get("name") or f"Track {meta.get('jamendo_id', cid)}",
        "artist": meta.get("artist", "—"),
        "duration": meta.get("duration", 0),
        "audioUrl": data_loader.audio_url(cid),
        "matchBreakdown": [],
        "explanation": "",
        "cyaniteScore": 1.0,
        "tagSim": 1.0,
        "finalScore": 1.0,
        "valence": va[0],
        "arousal": va[1],
        "autoDescription": "",
        "representativeSegmentStart": 0,
    }
