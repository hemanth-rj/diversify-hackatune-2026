import csv
import os
import random

_DATA = os.path.join(os.path.dirname(__file__), "..", "data")

TRACKS: dict[str, dict] = {}
CYANITE_TO_JAMENDO: dict[str, str] = {}
JAMENDO_TO_CYANITE: dict[str, str] = {}

def _load_from_db() -> bool:
    """Query the DB for all 50k tracks with names via jamendo_track join."""
    db_url = os.environ.get("DATABASE_URL", "")
    if not db_url:
        return False
    try:
        import psycopg2
        conn = psycopg2.connect(db_url)
        cur = conn.cursor()
        cur.execute("""
            SELECT t.cyanite_id, t.jamendo_id,
                   COALESCE(t.name, jt.name)            AS name,
                   COALESCE(t.artist, jt.artist_name)  AS artist,
                   COALESCE(t.duration_csv, jt.duration) AS duration
            FROM tracks t
            LEFT JOIN jamendo_track jt ON t.jamendo_id = jt.jamendo_id
            WHERE t.cyanite_id IS NOT NULL AND t.jamendo_id IS NOT NULL
        """)
        for cid, jid, name, artist, duration in cur.fetchall():
            TRACKS[cid] = {
                "jamendo_id": jid,
                "name": name or f"Track {jid}",
                "artist": artist or "—",
                "duration": int(duration) if duration else 0,
            }
            CYANITE_TO_JAMENDO[cid] = jid
            JAMENDO_TO_CYANITE[jid] = cid
        cur.close()
        conn.close()
        print(f"[data_loader] loaded {len(TRACKS)} tracks from DB")
        return True
    except Exception as e:
        print(f"[data_loader] DB load failed: {e}, falling back to CSV")
        return False


def _load_from_csv() -> None:
    with open(os.path.join(_DATA, "tracks.csv")) as f:
        for row in csv.DictReader(f):
            cid = row["cyanite_id"]
            jid = row["track_id"]
            TRACKS[cid] = {
                "jamendo_id": jid,
                "name": row["name"],
                "artist": row["artist_name"],
                "duration": int(row["duration"]) if row.get("duration") else 0,
            }
            CYANITE_TO_JAMENDO[cid] = jid
            JAMENDO_TO_CYANITE[jid] = cid
    print(f"[data_loader] loaded {len(TRACKS)} tracks from CSV")


if not _load_from_db():
    _load_from_csv()

USERS: dict[str, list[str]] = {}

with open(os.path.join(_DATA, "users.csv")) as f:
    for row in csv.DictReader(f):
        uid = row["user_id"]
        jids = row["liked_track_ids"].split()
        cids = [JAMENDO_TO_CYANITE[j] for j in jids if j in JAMENDO_TO_CYANITE]
        if cids:
            USERS[uid] = cids

USER_IDS: list[str] = sorted(USERS.keys())


def sample_seeds(user_id: str, n: int = 10) -> list[str]:
    cids = USERS.get(user_id, [])
    return random.sample(cids, min(n, len(cids))) if len(cids) > n else cids


_AUDIO_CORPUS = os.path.expanduser("~/mml-hackatune-26/harvester/corpus/audio")
_LOCAL_AUDIO = os.path.isdir(_AUDIO_CORPUS)


def audio_url(cyanite_id: str) -> str:
    jid = CYANITE_TO_JAMENDO.get(cyanite_id)
    if not jid:
        return ""
    if _LOCAL_AUDIO:
        return f"/audio/{jid}.mp3"
    return f"https://prod-1.storage.jamendo.com/download/track/{jid}/mp32/"
