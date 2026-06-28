#!/usr/bin/env python3
"""Fetch Cyanite model outputs for the LIKED tracks through our own gateway
(cached + typed + quota-tracked) and store them in queryable Postgres tables.

Cyanite is the main objective: this is the semantic layer (genre/mood/character/
instruments/valence-arousal/bpm/key/era/description) the experiments run on.
Tracks are fetched most-liked-first so coverage of real user taste comes online
fastest. Re-runnable: skips tracks already stored.
"""
import os, csv, json, time, urllib.request, urllib.error, collections
import psycopg2, psycopg2.extras

DSN = os.environ["DATABASE_URL"]
GW = os.environ.get("GATEWAY", "http://127.0.0.1:8080")
DATA = os.path.expanduser("~/mml-hackatune-26/data")
MODELS = ["MainGenreV2", "SubgenreV2", "MoodSimpleV2", "MoodAdvancedV2", "CharacterV2",
          "MovementV2", "InstrumentsV2", "VocalsV2", "ValenceArousalV2", "BpmV2", "KeyV2",
          "TimeSignatureV2", "FreeGenreV3", "MusicForV1", "MusicalEraV2", "AutoDescriptionV2"]
TAXO = {"MainGenreV2", "SubgenreV2", "MoodSimpleV2", "MoodAdvancedV2", "CharacterV2",
        "MovementV2", "InstrumentsV2", "VocalsV2", "FreeGenreV3", "MusicForV1"}

conn = psycopg2.connect(DSN)
conn.autocommit = True
cur = conn.cursor()
cur.execute("""CREATE TABLE IF NOT EXISTS cyanite_track(
    cyanite_id text PRIMARY KEY, bpm int, key text, time_signature text,
    valence real, arousal real, energy_level text, emotion_profile text,
    era int, vocal_presence text, vocal_gender text, description text,
    fetched_at timestamptz NOT NULL DEFAULT now())""")
cur.execute("""CREATE TABLE IF NOT EXISTS cyanite_tag(
    cyanite_id text, model text, tag text, score real,
    PRIMARY KEY(cyanite_id, model, tag))""")

# popularity of each jamendo id from real user likes
likes = collections.Counter()
for r in csv.reader(open(f"{DATA}/users.csv")):
    if r and r[0] != "user_id" and len(r) >= 2:
        for j in r[1].split():
            likes[j] += 1
# map jamendo -> cyanite via the DB
cur.execute("SELECT jamendo_id, cyanite_id FROM tracks WHERE jamendo_id IS NOT NULL")
j2c = {j: c for j, c in cur.fetchall()}
# already-fetched
cur.execute("SELECT cyanite_id FROM cyanite_track")
have = {r[0] for r in cur.fetchall()}
# liked cyanite ids, most-liked first, not yet fetched
ranked = sorted({j2c[j] for j in likes if j in j2c} - have,
                key=lambda c: -max(likes[j] for j in likes if j2c.get(j) == c))
# HARD CAP on total Cyanite-tagged tracks — the tagging quota is pooled across all
# teams, so never fetch more than MAX_TRACKS in total (already-cached ones are free).
MAX = int(os.environ.get("MAX_TRACKS", "1000"))
ranked = ranked[:max(0, MAX - len(have))]
print(f"cyanite fetch: cap {MAX} total ({len(have)} cached, fetching {len(ranked)} more)", flush=True)

qs = "&".join(f"model={m}" for m in MODELS)
n = 0
for cid in ranked:
    env = None
    for attempt in range(6):
        try:
            with urllib.request.urlopen(f"{GW}/v1/library-tracks/{cid}/models?{qs}", timeout=60) as r:
                env = json.load(r)
            break
        except urllib.error.HTTPError as e:
            if e.code == 429:  # shared-key rate limit (other teams) — wait it out
                time.sleep(4 * (attempt + 1))
                continue
            print(f"skip {cid}: {e}", flush=True)
            break
        except Exception as e:
            print(f"skip {cid}: {e}", flush=True)
            break
    if env is None:
        continue
    if not isinstance(env, dict) or "items" not in env:
        continue
    sc = {}  # scalar fields for cyanite_track
    tags = []  # (model, tag, score)
    for m in env["items"]:
        v = m.get("version")
        if v == "BpmV2":
            sc["bpm"] = m.get("tag")
        elif v == "KeyV2":
            sc["key"] = m.get("tag")
        elif v == "TimeSignatureV2":
            sc["time_signature"] = m.get("tag")
        elif v == "ValenceArousalV2":
            s = m.get("scores", {})
            sc["valence"] = s.get("valence"); sc["arousal"] = s.get("arousal")
            sc["energy_level"] = m.get("energyLevel"); sc["emotion_profile"] = m.get("emotionProfile")
        elif v == "MusicalEraV2":
            sc["era"] = m.get("estimatedProductionYear")
        elif v == "AutoDescriptionV2":
            sc["description"] = m.get("description")
        elif v == "VocalsV2":
            sc["vocal_presence"] = m.get("vocalPresence"); sc["vocal_gender"] = m.get("predominantVocalGender")
        if v in TAXO:
            scores = m.get("scores") or {}
            for t in (m.get("tags") or []):
                tags.append((v, t, scores.get(t)))
    cur.execute("""INSERT INTO cyanite_track(cyanite_id,bpm,key,time_signature,valence,arousal,
        energy_level,emotion_profile,era,vocal_presence,vocal_gender,description)
        VALUES(%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s) ON CONFLICT (cyanite_id) DO NOTHING""",
        (cid, sc.get("bpm"), sc.get("key"), sc.get("time_signature"), sc.get("valence"),
         sc.get("arousal"), sc.get("energy_level"), sc.get("emotion_profile"), sc.get("era"),
         sc.get("vocal_presence"), sc.get("vocal_gender"), sc.get("description")))
    for (model, tag, score) in tags:
        cur.execute("INSERT INTO cyanite_tag(cyanite_id,model,tag,score) VALUES(%s,%s,%s,%s) ON CONFLICT DO NOTHING",
                    (cid, model, tag, score))
    n += 1
    if n % 100 == 0:
        print(f"cyanite: {n}/{len(ranked)} fetched", flush=True)
print(f"cyanite fetch complete: {n} tracks", flush=True)
