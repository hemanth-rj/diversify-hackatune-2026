from fastapi import APIRouter, HTTPException
from pydantic import BaseModel
import llm, db_search
import learned_weights as _lw
import idf as _idf

router = APIRouter()


class Message(BaseModel):
    role: str
    content: str


class ChatRequest(BaseModel):
    messages: list[Message]
    limit: int = 10


@router.post("/chat")
def chat(req: ChatRequest):
    messages = [{"role": m.role, "content": m.content} for m in req.messages]
    intent = llm.parse_search_intent(messages)
    query = intent.get("query", "")
    meta_filter = intent.get("metadataFilter") or {}
    summary = intent.get("summary") or query or "Searching…"

    if not query:
        raise HTTPException(status_code=400, detail="Could not parse search intent")

    print(f"[chat] query={query!r} filter={meta_filter}")

    weights = _lw.LEARNED_WEIGHTS if _lw.LEARNED_WEIGHTS is not None else _idf.IDF_WEIGHTS

    # Tier 1: strict AND across all models
    tracks = db_search.search_by_filter(meta_filter, weights, limit=req.limit, strict=True)

    # Tier 2: relax to OR (any one model matches)
    if not tracks and meta_filter:
        tracks = db_search.search_by_filter(meta_filter, weights, limit=req.limit, strict=False)
        if tracks:
            summary = f"{summary} · broadened to partial match"

    # Tier 3: no filter at all — score full catalog by learned weights
    if not tracks:
        tracks = db_search.score_catalog(weights, limit=req.limit)
        summary = f"{summary} · showing top picks by taste profile"

    return {
        "tracks": tracks,
        "inferredQuery": query,
        "inferredFilters": meta_filter,
        "filterSummary": summary,
    }
