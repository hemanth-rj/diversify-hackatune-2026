"""Endpoint for learned weights analysis visualization."""
import json, os
from fastapi import APIRouter, HTTPException

router = APIRouter()

_PATH = os.path.join(os.path.dirname(__file__), "..", "data", "learned_weights.json")

_CACHE: dict | None = None


def _load() -> dict:
    global _CACHE
    if _CACHE is not None:
        return _CACHE
    if not os.path.exists(_PATH):
        raise HTTPException(status_code=503, detail="learned_weights.json not found")
    with open(_PATH) as f:
        d = json.load(f)

    labels: list[str] = d["dim_labels"]
    weights: list[float] = d["weights"]
    total = sum(weights) + 1e-12

    def category(label: str) -> str:
        if label.startswith("InstrumentsV2"): return "Instruments"
        if label.startswith("ValenceArousal"): return "ValenceArousal"
        if label.startswith("MoodSimple"): return "Mood"
        if label.startswith("MainGenre"): return "Genre"
        if label.startswith("Bpm"): return "BPM"
        return "Other"

    dims = sorted(
        [
            {
                "label": lbl,
                "category": category(lbl),
                "shortLabel": lbl.split(".")[-1] if "." in lbl else lbl,
                "weight": round(float(w), 6),
                "pct": round(100.0 * float(w) / total, 3),
            }
            for lbl, w in zip(labels, weights)
        ],
        key=lambda x: -x["weight"],
    )

    cat_totals: dict[str, float] = {}
    for dim in dims:
        cat_totals.setdefault(dim["category"], 0.0)
        cat_totals[dim["category"]] += dim["weight"]

    category_totals = sorted(
        [
            {"name": k, "pct": round(100.0 * v / total, 1), "total": round(float(v), 6)}
            for k, v in cat_totals.items()
        ],
        key=lambda x: -x["pct"],
    )

    _CACHE = {
        "n_folds": d["n_folds"],
        "n_users": d["n_users"],
        "dims": dims,
        "category_totals": category_totals,
    }
    return _CACHE


@router.get("/weights")
def weights():
    return _load()
