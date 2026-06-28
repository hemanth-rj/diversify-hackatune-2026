import numpy as np
import idf as _idf

MOOD_KEYS = [
    "aggressive", "calm", "chill", "dark", "energetic", "epic",
    "happy", "romantic", "sad", "scary", "sexy", "ethereal", "uplifting",
]
GENRE_KEYS = [
    "african", "ambient", "middleEastern", "asian", "blues", "childrenJingle",
    "classical", "electronic", "folkCountry", "funkSoul", "indian", "jazz",
    "latin", "metal", "pop", "rapHipHop", "reggae", "rnb", "rock",
    "singerSongwriter", "sound", "soundtrack", "spokenWord",
]
INSTRUMENT_KEYS = [
    "accordion", "acousticGuitar", "africanPercussion", "asianFlute", "asianStrings",
    "banjo", "bass", "bassGuitar", "bells", "bongoConga", "brass", "celeste",
    "cello", "churchOrgan", "clarinet", "doubleBass", "drumKit", "electricGuitar",
    "electricOrgan", "electricPiano", "electronicDrums", "flute", "horn",
    "glockenspiel", "harp", "harpsichord", "luteOud", "mandolin", "marimba",
    "oboe", "percussion", "piano", "pizzicato", "saxophone", "sitar", "steelDrums",
    "strings", "synth", "tabla", "taiko", "trumpet", "tuba", "ukulele",
    "vibraphone", "violin", "whistling", "woodwinds",
]
DIM_LABELS = (
    [f"MoodSimpleV2.{k}" for k in MOOD_KEYS] +
    [f"MainGenreV2.{k}" for k in GENRE_KEYS] +
    [f"InstrumentsV2.{k}" for k in INSTRUMENT_KEYS] +
    ["ValenceArousalV2.valence", "ValenceArousalV2.arousal", "BpmV2.bpm"]
)  # length 86

_MOOD_OFF = 0
_GENRE_OFF = 13
_INSTR_OFF = 36
_VALENCE = 83
_AROUSAL = 84
_BPM = 85

# InstrumentsV2 uses presence categories (not scores)
_PRESENCE = {"absent": 0.0, "partially": 0.33, "frequently": 0.67, "throughout": 1.0}


def build_tag_vector(tag_outputs: dict) -> np.ndarray:
    vec = np.zeros(86, dtype=np.float32)

    mood = tag_outputs.get("MoodSimpleV2", {})
    scores = mood.get("scores", {})
    for i, k in enumerate(MOOD_KEYS):
        vec[_MOOD_OFF + i] = float(scores.get(k, 0.0))

    genre = tag_outputs.get("MainGenreV2", {})
    scores = genre.get("scores", {})
    for i, k in enumerate(GENRE_KEYS):
        vec[_GENRE_OFF + i] = float(scores.get(k, 0.0))

    # InstrumentsV2 has no `scores` — uses `presence` dict with ordinal strings
    instruments = tag_outputs.get("InstrumentsV2", {})
    presence = instruments.get("presence", {})
    for i, k in enumerate(INSTRUMENT_KEYS):
        vec[_INSTR_OFF + i] = _PRESENCE.get(presence.get(k, "absent"), 0.0)

    # ValenceArousalV2 track-level scores are under `scores` sub-dict, range -1..1
    va_scores = tag_outputs.get("ValenceArousalV2", {}).get("scores", {})
    vec[_VALENCE] = (float(va_scores.get("valence", 0.0) or 0.0) + 1.0) / 2.0
    vec[_AROUSAL] = (float(va_scores.get("arousal", 0.0) or 0.0) + 1.0) / 2.0

    bpm = tag_outputs.get("BpmV2", {})
    vec[_BPM] = min(float(bpm.get("tag", 0) or 0) / 300.0, 1.0)

    return vec


def cosine_sim(a: np.ndarray, b: np.ndarray) -> float:
    na, nb = np.linalg.norm(a), np.linalg.norm(b)
    if na < 1e-9 or nb < 1e-9:
        return 0.0
    return float(np.dot(a, b) / (na * nb))


def weighted_cosine_sim(a: np.ndarray, b: np.ndarray, weights: np.ndarray) -> float:
    aw, bw = a * weights, b * weights
    na, nb = np.linalg.norm(aw), np.linalg.norm(bw)
    if na < 1e-9 or nb < 1e-9:
        return 0.0
    return float(np.dot(aw, bw) / (na * nb))


def build_taste_profile(liked_vectors: list[np.ndarray]) -> dict:
    idf_weights = _idf.IDF_WEIGHTS
    M = np.stack(liked_vectors)
    mean_vec = M.mean(axis=0)
    var = M.var(axis=0)
    inv_var = 1.0 / (var + 1e-6)
    combined = inv_var * idf_weights
    total = combined.sum()
    weights = combined / (total + 1e-9)
    return {"mean": mean_vec, "weights": weights}


def build_taste_profile_with_holdout(
    seed_vectors: list[np.ndarray],
    held_out_vectors: list[np.ndarray],
) -> dict:
    """
    Calibrate re-ranking weights using held-out liked tracks as a validation signal.

    Dimensions that score high in BOTH seed tracks (used for search) and held-out
    liked tracks (withheld from the search) are the ones that reliably predict this
    user's taste. We amplify those dimensions and down-weight ones that appeared
    only in the seeds or only in the held-out set.

    Combined with IDF: dimensions that are rare in the catalog AND consistently high
    across the user's full library (seeds + held-out) get the highest weight.

    Falls back to inverse-variance × IDF if no held-out vectors are available.
    """
    idf_weights = _idf.IDF_WEIGHTS
    M = np.stack(seed_vectors)
    seed_mean = M.mean(axis=0)

    if held_out_vectors:
        held_out_mean = np.mean(np.stack(held_out_vectors), axis=0)
        # Element-wise product: amplified only where both are high
        calibration = seed_mean * held_out_mean
    else:
        var = M.var(axis=0)
        calibration = (1.0 / (var + 1e-6)) * seed_mean

    combined = calibration * idf_weights
    total = combined.sum()
    weights = combined / (total + 1e-9)
    return {"mean": seed_mean, "weights": weights}


def rerank_global(
    results: list[dict],
    cyanite_scores: list[float],
    weights: np.ndarray,
    alpha: float = 0.5,
) -> list[dict]:
    """
    Re-rank without a seed vector. Score = dot(tag_vector, weights).
    Used when there is no user taste profile (Chat, MoodBoard).
    Breakdown shows top dims by weight × tag score (no delta — no seed to compare against).
    """
    top_weight_idx = list(np.argsort(-weights)[:20])

    for res, c_score in zip(results, cyanite_scores):
        tv = res.get("tag_vector")
        if tv is None:
            res["tag_sim"] = 0.0
            res["final_score"] = round((1 - alpha) * c_score, 3)
            res["match_breakdown"] = []
            continue
        tag_score = float(np.dot(tv, weights))
        final = alpha * tag_score + (1 - alpha) * c_score

        # Show top-3 dims among high-weight dimensions where this track scores > 5%
        candidates = [
            (j, float(tv[j]))
            for j in top_weight_idx
            if float(tv[j]) > 0.05
        ]
        candidates.sort(key=lambda x: -x[1])
        breakdown = [
            {
                "dimension": DIM_LABELS[j],
                "seedScore": round(float(tv[j]), 3),
                "resultScore": round(float(tv[j]), 3),
                "delta": 0.0,
            }
            for j, _ in candidates[:3]
        ]

        res["tag_sim"] = round(tag_score, 3)
        res["final_score"] = round(final, 3)
        res["match_breakdown"] = breakdown
    return sorted(results, key=lambda r: r.get("final_score", 0), reverse=True)


def rerank(
    seed_vectors: list[np.ndarray],
    results: list[dict],
    cyanite_scores: list[float],
    alpha: float = 0.5,
    weights: np.ndarray | None = None,
) -> list[dict]:
    if not results:
        return results

    seed_mean = np.stack(seed_vectors).mean(axis=0)

    # For match_breakdown: pick candidate dims by weight (or uniform), then sort by smallest delta
    if weights is not None:
        top_weight_idx = set(np.argsort(-weights)[:20])
    else:
        top_weight_idx = set(range(86))

    for res, c_score in zip(results, cyanite_scores):
        tv = res.get("tag_vector")
        if tv is None:
            res["tag_sim"] = 0.0
            res["final_score"] = round((1 - alpha) * c_score, 3)
            res["match_breakdown"] = []
            continue

        if weights is not None:
            tag_sim = weighted_cosine_sim(seed_mean, tv, weights)
        else:
            tag_sim = cosine_sim(seed_mean, tv)

        final = alpha * tag_sim + (1 - alpha) * c_score

        # Top-3 agreeing dims: among highest-weighted, pick smallest delta where both > 0
        candidates = [
            (j, abs(float(seed_mean[j]) - float(tv[j])))
            for j in top_weight_idx
            if float(seed_mean[j]) > 0.05 and float(tv[j]) > 0.05
        ]
        candidates.sort(key=lambda x: x[1])
        breakdown = [
            {
                "dimension": DIM_LABELS[j],
                "seedScore": round(float(seed_mean[j]), 3),
                "resultScore": round(float(tv[j]), 3),
                "delta": round(delta, 3),
            }
            for j, delta in candidates[:3]
        ]

        res["tag_sim"] = round(tag_sim, 3)
        res["final_score"] = round(final, 3)
        res["match_breakdown"] = breakdown

    return sorted(results, key=lambda r: r.get("final_score", 0), reverse=True)
