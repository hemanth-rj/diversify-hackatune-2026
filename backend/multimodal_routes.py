from fastapi import APIRouter, HTTPException
from pydantic import BaseModel
import llm, db_search
import learned_weights as _lw
import idf as _idf

router = APIRouter()


class MultimodalRequest(BaseModel):
    brief: str | None = None
    image: str | None = None
    mimeType: str = "image/jpeg"
    limit: int = 10


@router.post("/multimodal")
def multimodal(req: MultimodalRequest):
    if not req.brief and not req.image:
        raise HTTPException(status_code=400, detail="Provide 'brief' or 'image'")

    if req.image:
        intent = llm.describe_image_for_music(req.image, req.mimeType)
    else:
        intent = llm.describe_brief_for_music(req.brief)

    query = intent.get("query", "")
    meta_filter = intent.get("metadataFilter") or {}
    summary = intent.get("summary", query)

    print(f"[multimodal] query={query!r} filter={meta_filter}")

    weights = _lw.LEARNED_WEIGHTS if _lw.LEARNED_WEIGHTS is not None else _idf.IDF_WEIGHTS
    tracks = db_search.search_by_filter(meta_filter, weights, limit=req.limit)

    return {
        "tracks": tracks,
        "inferred": {"query": query, "filterSummary": summary},
    }
