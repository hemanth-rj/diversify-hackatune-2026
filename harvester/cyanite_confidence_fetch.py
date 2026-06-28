#!/usr/bin/env python3
"""Capture the Cyanite CONFIDENCE struct for the musical-parameter models
(BpmV2 / KeyV2 / TimeSignatureV2) by RE-PARSING the already-cached /models
responses in cyanite_cache. ZERO Cyanite quota — reads the local cache only.

Why this exists: the original harvester/cyanite_fetch.py kept only the *value*
(bpm/key/time_signature) and DROPPED the confidence. It also looked for a field
named `confidence`, but the Cyanite REST payload actually nests the struct under
`confidences` (plural):
    {"version":"BpmV2","tag":60,
     "confidences":{"modelCertainty":0.94,"predictionStability":0.98,"confidence":0.94}}
so even a re-parse for `confidence` would have missed it. We read `confidences`.

Idempotent (ON CONFLICT upsert). Run with --dry-run to count without writing.
"""
import os, re, sys, json, psycopg2

DSN = os.environ.get("DATABASE_URL", "postgres://harvest:harvest@localhost:5432/harvest")
CONF_MODELS = {"BpmV2", "KeyV2", "TimeSignatureV2"}

DDL = """
CREATE TABLE IF NOT EXISTS cyanite_confidence(
    cyanite_id text NOT NULL,
    model      text NOT NULL,         -- BpmV2 | KeyV2 | TimeSignatureV2
    certainty  real,                  -- modelCertainty   (how sure the model is)
    stability  real,                  -- predictionStability (steadiness across the track)
    confidence real,                  -- the headline confidence Cyanite reports
    PRIMARY KEY(cyanite_id, model));
CREATE INDEX IF NOT EXISTS cyanite_confidence_model_idx ON cyanite_confidence(model, confidence);
"""

_CID = re.compile(r"library-tracks/([^/?]+)/models")


def cid_from_path(path):
    m = _CID.search(path or "")
    return m.group(1) if m else None


def collect(o, out):
    """Walk the model-output JSON; record the `confidences` struct for the 3 conf models."""
    if isinstance(o, dict):
        v = o.get("version")
        cf = o.get("confidences")
        if v in CONF_MODELS and isinstance(cf, dict):
            out[v] = (cf.get("modelCertainty"), cf.get("predictionStability"), cf.get("confidence"))
        for x in o.values():
            collect(x, out)
    elif isinstance(o, list):
        for x in o:
            collect(x, out)


def main():
    dry = "--dry-run" in sys.argv
    c = psycopg2.connect(DSN)
    cur = c.cursor()
    if not dry:
        cur.execute(DDL)
        c.commit()
    cur.execute("SELECT path, body FROM cyanite_cache WHERE action='tagging'")
    rows = cur.fetchall()
    wcur = c.cursor()
    tracks, upserts, no_cid, by_model = 0, 0, 0, {}
    for path, body in rows:
        cid = cid_from_path(path)
        if not cid:
            no_cid += 1
            continue
        try:
            d = json.loads(body)
        except Exception:
            continue
        out = {}
        collect(d, out)
        if not out:
            continue
        tracks += 1
        for model, (cert, stab, conf) in out.items():
            by_model[model] = by_model.get(model, 0) + 1
            if dry:
                continue
            wcur.execute(
                "INSERT INTO cyanite_confidence(cyanite_id,model,certainty,stability,confidence) "
                "VALUES(%s,%s,%s,%s,%s) ON CONFLICT(cyanite_id,model) DO UPDATE SET "
                "certainty=EXCLUDED.certainty, stability=EXCLUDED.stability, confidence=EXCLUDED.confidence",
                (cid, model, cert, stab, conf))
            upserts += 1
    if not dry:
        c.commit()
    print(f"{'DRY-RUN: ' if dry else ''}scanned {len(rows)} cached /models responses; "
          f"{tracks} tracks carried confidences; per-model {by_model}; "
          f"{'would upsert' if dry else 'upserted'} {upserts if not dry else sum(by_model.values())} rows; "
          f"{no_cid} rows had no cid in path.")
    if not dry:
        cur.execute("SELECT model, count(*), round(avg(confidence)::numeric,3), round(avg(certainty)::numeric,3) "
                    "FROM cyanite_confidence GROUP BY model ORDER BY model")
        for m, n, ac, acert in cur.fetchall():
            print(f"  {m}: {n} rows · avg confidence {ac} · avg certainty {acert}")


if __name__ == "__main__":
    main()
