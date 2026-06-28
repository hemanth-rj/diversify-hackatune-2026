#!/usr/bin/env python3
"""Populate cyanite_arousal_traj — per-track ValenceArousalV2 emotion TRAJECTORIES
(segment-level valence/arousal time-series) — with ZERO new quota where possible.

Strategy (per the TRAJECTORIES analysis):
  1. FIRST parse ValenceArousalV2 `segments` straight out of the gateway's already
     cached /models responses in `cyanite_cache` (path '.../models', status 200).
     This costs NOTHING — the bytes were already paid for by a previous tagging call.
  2. ONLY for the top-300 most-liked tracks that still lack a trajectory AND are NOT
     buildable from cache do we do a BOUNDED refetch through the gateway — one
     `GET /v1/library-tracks/{id}/models?model=ValenceArousalV2` each. The gateway
     caches + atomically debits the pooled tagging quota, so a repeat run is free.

Idempotent: upserts ON CONFLICT DO NOTHING, skips tracks that already have a row,
and never refetches a track already satisfiable from cache. Safe to run repeatedly.

  --dry-run : report buildable-from-cache vs would-refetch counts, spend ZERO quota,
              write NOTHING. Run this first to see exactly what a live run would do.

Cyanite ValenceArousalV2 segment shape (guides/model_outputs.md):
  { "version": "ValenceArousalV2",
    "scores": {"valence": .., "arousal": ..},
    "segments": { "timestampsSeconds": [5.0, 15.0, ...],
                  "values": { "valence": [..], "arousal": [..] } }, ... }
The /models envelope wraps these as {"items": [ <model output>, ... ]}.
"""
import os, sys, csv, json, time, urllib.request, urllib.error, collections
import psycopg2

DSN = os.environ["DATABASE_URL"]
GW = os.environ.get("GATEWAY", "http://127.0.0.1:8080")
DATA = os.path.expanduser(os.environ.get("HACKATUNE_DATA", "~/mml-hackatune-26/data"))
MODEL = "ValenceArousalV2"
MAX_REFETCH = int(os.environ.get("MAX_REFETCH", "300"))   # bounded quota spend cap
DRY = "--dry-run" in sys.argv


def _parse_va_segments(env):
    """Extract (ts, valence, arousal) float lists from a /models response envelope.

    `env` is the parsed JSON body of a GET .../models response (dict with "items",
    or a bare list of model outputs). Returns (ts, valence, arousal) as equal-length
    lists of floats, or None if no usable ValenceArousalV2 segment series is present
    (missing/null segments, missing axes, or mismatched array lengths).
    """
    if env is None:
        return None
    items = env.get("items") if isinstance(env, dict) else env
    if not isinstance(items, list):
        return None
    for m in items:
        if not isinstance(m, dict) or m.get("version") != MODEL:
            continue
        seg = m.get("segments")
        if not isinstance(seg, dict):
            return None
        ts = seg.get("timestampsSeconds")
        vals = seg.get("values")
        if not isinstance(ts, list) or not isinstance(vals, dict):
            return None
        val = vals.get("valence")
        aro = vals.get("arousal")
        if not isinstance(val, list) or not isinstance(aro, list):
            return None
        n = len(ts)
        if n == 0 or len(val) != n or len(aro) != n:
            return None
        try:
            ts = [float(x) for x in ts]
            val = [float(x) for x in val]
            aro = [float(x) for x in aro]
        except (TypeError, ValueError):
            return None
        return ts, val, aro
    return None  # no ValenceArousalV2 item in this envelope


def _cid_from_path(path):
    """library-tracks/{id}/models -> {id}."""
    parts = [p for p in (path or "").split("/") if p]
    if len(parts) >= 2 and parts[-1] == "models":
        return parts[-2]
    return None


def main():
    conn = psycopg2.connect(DSN)
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("""CREATE TABLE IF NOT EXISTS cyanite_arousal_traj(
        cyanite_id text, model text, ts real[], arousal real[], valence real[],
        PRIMARY KEY(cyanite_id, model))""")

    # ---- who already has a trajectory (skip these) ----
    cur.execute("SELECT cyanite_id FROM cyanite_arousal_traj WHERE model=%s", (MODEL,))
    have = {r[0] for r in cur.fetchall()}

    # ---- 1) scan ALREADY-cached /models responses (zero quota) ----
    cur.execute("""SELECT path, body FROM cyanite_cache
                   WHERE status=200 AND path LIKE '%%/models' AND body LIKE %s""",
                ("%" + MODEL + "%",))
    cache_buildable = {}   # cid -> (ts, val, aro)
    for path, body in cur.fetchall():
        cid = _cid_from_path(path)
        if not cid or cid in have or cid in cache_buildable:
            continue
        try:
            env = json.loads(body)
        except (ValueError, TypeError):
            continue
        parsed = _parse_va_segments(env)
        if parsed is not None:
            cache_buildable[cid] = parsed

    # ---- popularity ranking of liked tracks (mirror cyanite_fetch.py) ----
    likes = collections.Counter()
    try:
        with open(f"{DATA}/users.csv", newline="") as f:
            for r in csv.reader(f):
                if r and r[0] != "user_id" and len(r) >= 2:
                    for j in r[1].split():
                        likes[j] += 1
    except FileNotFoundError:
        print(f"warn: {DATA}/users.csv not found; refetch candidate list will be empty", flush=True)
    cur.execute("SELECT jamendo_id, cyanite_id FROM tracks WHERE jamendo_id IS NOT NULL")
    j2c = {j: c for j, c in cur.fetchall()}
    c_pop = collections.Counter()
    for j, n in likes.items():
        c = j2c.get(j)
        if c:
            c_pop[c] += n
    liked_ranked = [c for c, _ in c_pop.most_common()]

    # ---- 2) bounded refetch candidates: most-liked, no traj, NOT in cache ----
    refetch_candidates = [c for c in liked_ranked
                          if c not in have and c not in cache_buildable][:MAX_REFETCH]

    print(f"trajectories: have={len(have)} already | "
          f"buildable_from_cache={len(cache_buildable)} | "
          f"liked_without_traj={sum(1 for c in liked_ranked if c not in have)} | "
          f"would_refetch={len(refetch_candidates)} (cap {MAX_REFETCH})", flush=True)

    if DRY:
        print(f"[DRY-RUN] no quota spent, nothing written. "
              f"A live run would build {len(cache_buildable)} from cache (free) and "
              f"refetch up to {len(refetch_candidates)} via gateway (<= {len(refetch_candidates)} tagging units).",
              flush=True)
        print(f"SUMMARY built=0 from_cache={len(cache_buildable)} "
              f"refetched=0 skipped={len(refetch_candidates)} (dry-run)", flush=True)
        return

    def upsert(cid, ts, val, aro):
        cur.execute("""INSERT INTO cyanite_arousal_traj(cyanite_id, model, ts, arousal, valence)
                       VALUES(%s,%s,%s,%s,%s) ON CONFLICT DO NOTHING""",
                    (cid, MODEL, ts, aro, val))
        return cur.rowcount or 0

    # ---- write everything buildable from cache (free) ----
    from_cache = 0
    for cid, (ts, val, aro) in cache_buildable.items():
        from_cache += upsert(cid, ts, val, aro)

    # ---- bounded refetch via the gateway (caches + debits quota) ----
    refetched = skipped = 0
    qs = f"model={MODEL}"
    for cid in refetch_candidates:
        env = None
        for attempt in range(6):
            try:
                req = f"{GW}/v1/library-tracks/{cid}/models?{qs}"
                with urllib.request.urlopen(req, timeout=60) as r:
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
        parsed = _parse_va_segments(env)
        if parsed is None:
            skipped += 1
            continue
        ts, val, aro = parsed
        refetched += upsert(cid, ts, val, aro)
        if (refetched + skipped) % 50 == 0:
            print(f"refetch progress: {refetched + skipped}/{len(refetch_candidates)}", flush=True)

    built = from_cache + refetched
    print(f"SUMMARY built={built} from_cache={from_cache} "
          f"refetched={refetched} skipped={skipped}", flush=True)


if __name__ == "__main__":
    main()
