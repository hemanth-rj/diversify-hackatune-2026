import os, time, requests as req

# Route through the shared caching gateway — key is injected server-side,
# responses are cached so repeated calls don't burn the shared event quota.
BASE = "http://localhost:8080/v1"
MODELS = [
    "MainGenreV2", "MoodSimpleV2", "InstrumentsV2", "CharacterV2",
    "ValenceArousalV2", "BpmV2", "AutoDescriptionV2", "RepresentativeSegmentV2",
]

_session = None

def _get_session() -> req.Session:
    global _session
    if _session is None:
        _session = req.Session()
        # Gateway injects x-api-key server-side; no auth header needed from client
    return _session


def _call(method: str, url: str, **kwargs) -> dict:
    s = _get_session()
    for attempt in range(2):
        r = getattr(s, method)(url, timeout=30, **kwargs)
        if r.status_code == 429:
            time.sleep(2)
            continue
        r.raise_for_status()
        return r.json()
    r.raise_for_status()


def search_by_prompt(query: str, limit: int = 20,
                     metadata_filter: dict | None = None) -> list[dict]:
    body = {"query": query}
    if metadata_filter:
        body["metadataFilter"] = metadata_filter
    data = _call("post", f"{BASE}/private-alpha/library-tracks/search",
                 params={"limit": limit}, json=body)
    return data.get("items", [])


def find_similar(track_id: str, limit: int = 20,
                 metadata_filter: dict | None = None) -> list[dict]:
    body = {}
    if metadata_filter:
        body["metadataFilter"] = metadata_filter
    data = _call("post", f"{BASE}/private-alpha/library-tracks/{track_id}/similar",
                 params={"limit": limit}, json=body)
    return data.get("items", [])


def find_similar_multi(track_ids: list[str], limit: int = 20,
                       metadata_filter: dict | None = None) -> list[dict]:
    body = {"tracks": [{"id": tid} for tid in track_ids]}
    if metadata_filter:
        body["metadataFilter"] = metadata_filter
    data = _call("post", f"{BASE}/private-alpha/library-tracks/similar",
                 params={"limit": limit}, json=body)
    return data.get("items", [])


def get_model_outputs(track_id: str, models: list[str] | None = None) -> dict:
    params = [("model", m) for m in (models or MODELS)]
    data = _call("get", f"{BASE}/library-tracks/{track_id}/models", params=params)
    result = {}
    for item in data.get("items", []):
        version = item.get("version")
        if version:
            result[version] = item
    return result
