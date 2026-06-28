import threading
import cyanite

_CACHE: dict[str, dict] = {}
_lock = threading.Lock()


def get_tags(cyanite_id: str) -> dict | None:
    return _CACHE.get(cyanite_id)


def store_tags(cyanite_id: str, tag_outputs: dict) -> None:
    with _lock:
        _CACHE[cyanite_id] = tag_outputs


def batch_fetch_missing(ids: list[str]) -> None:
    with _lock:
        missing = [cid for cid in ids if cid not in _CACHE]
    for cid in missing:
        try:
            outputs = cyanite.get_model_outputs(cid)
            store_tags(cid, outputs)
        except Exception as e:
            print(f"[cache] failed to fetch {cid}: {e}")
