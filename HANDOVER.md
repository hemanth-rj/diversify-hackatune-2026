# Sounds Like You — Handover

**Date:** June 2026  
**Event:** HACKATUNE 2026, Munich Music Labs × Cyanite  
**Live:** http://95.216.72.161:8001

---

## Current state

All five tabs are working and deployed. The system is stable enough for demo use.

| Component | Status | Notes |
|---|---|---|
| Chat | ✓ Working | 3-tier fallback; always returns tracks |
| Taste | ✓ Working | Top-30 users by likes; fast (numpy only) |
| Similar by name | ✓ Working | Tag + acoustic blend |
| Similar by audio file | ✓ Working (server only) | Rust binary; not available in local dev |
| MoodBoard | ✓ Working | Gemini Vision → filter → results |
| Insights | ✓ Working | Weight bar chart + category donut |
| Audio playback | ✓ Working | 50 k local MP3s served at /audio/; CDN fallback locally |
| Track modal | ✓ Working | Full Cyanite detail + audio features |

---

## Architecture decisions made during the hackathon

### No Cyanite API at runtime
The live webapp makes zero Cyanite API calls. All tags are pre-fetched into Postgres
(`cyanite_tag`, `cyanite_track`). This avoids the 180 req/min shared quota entirely
and makes responses fast.  
*Trade-off: new tracks added to Jamendo after ingestion won't have tags until the
harvester re-runs.*

### Learned weights over IDF
The initial approach used IDF (inverse document frequency) to down-weight common tags.
We added a LOO training pass (14,476 folds, 462 users) that learns dimension weights
from actual user taste data. The learned weights are loaded at startup and used for
all scoring. IDF is kept as fallback.  
*Trade-off: weights are a global average — they don't adapt per-user. Per-user fine-tuning
would require storing per-user weight vectors.*

### 3-tier search fallback
Chat used to silently return empty results when the LLM filter was too specific.
Now: strict AND → loose OR → full catalog. The filter summary label tells the user
which tier fired.  
*Trade-off: tier 3 results may feel unrelated to the query. Worth considering a
"nothing found" message as an alternative to the full-catalog fallback.*

### Rust binary called via subprocess
The "similar by audio file" endpoint shells out to the Rust `query similar-file` binary
rather than reimplementing 382-dim feature extraction in Python. The binary uses the
same Postgres connection, outputs JSON to stdout, and is read back by FastAPI.  
*Trade-off: subprocess adds ~200 ms overhead per upload and makes the feature
Linux/x86-64-only. Alternative: port feature extraction to Python using librosa,
which would be portable but slower (≈2× on extraction, same pgvector query).*

### StaticFiles audio serving
Audio was originally served from Jamendo's CDN (`prod-1.storage.jamendo.com`), causing
frequent "preview unavailable" failures as CDN links expired or were geo-restricted.
Now the 50 k MP3s are served directly by FastAPI from the local corpus.  
*Trade-off: adds ~4 GB corpus to the server's disk footprint. FastAPI StaticFiles
is single-threaded — for heavy concurrent audio traffic, put Nginx in front.*

### No per-fold metrics stored
`compute_learned_weights.py` stores only the final weight vector, not per-fold scores.
The "feature agreement" metric used for training is not a retrieval rank — we don't
have recall@K numbers without re-running the evaluation on the full catalog.  
*Trade-off: the Insights tab can't show validation accuracy. Storing per-fold rank
would require a full catalog scan per fold (expensive but feasible with numpy batching).*

---

## Known issues and rough edges

| Issue | Severity | Notes |
|---|---|---|
| Chat intent quality | Medium | Gemini occasionally returns empty `metadataFilter` for valid queries; tier-2/3 fallback covers it but results are loose |
| Audio preview unavailable (legacy links) | Low | Fixed — now local. Old CDN links in any cached frontend state will 404 once, then reload correctly |
| Similar-by-file not available locally | Low | Rust binary is Linux-only; not a blocker for local dev of other tabs |
| No authentication | Low | The API is open; any user can query any user's taste profile |
| MoodBoard requires good image | Medium | Gemini Vision struggles with abstract or text-heavy images |
| Tag coverage gap | Medium | Only ~10 k of 50 k tracks have Cyanite tags; other 40 k appear only in audio-similarity results |
| Single server, no load balancing | Low | The prewarm loads ~400 MB of tag vectors into RAM; a second instance would double that |

---

## If you want to extend this

### Add more tracks to the tagged catalog
Re-run the harvester on new Jamendo tracks (it's resumable and idempotent), then
run the Cyanite batch fetch script. The in-memory cache (`_ALL_VECTORS`) reloads
on next restart.

### Add per-user learned weights
Instead of one global weight vector, compute a weight vector per user during taste
recommendation (the `build_taste_profile_with_holdout` function in `rerank.py`
already implements this — it's just not wired to the learned_weights path).

### Replace Gemini with a local model
`llm.py` is the only Gemini dependency at runtime. It calls `parse_search_intent()`
and `describe_for_music()`. Replacing these with a local Ollama call or any
OpenAI-compatible endpoint is straightforward — just swap `_get_client()`.

### Put Nginx in front
For audio serving and SSL termination:
```nginx
server {
    listen 80;
    location /audio/ { alias /home/ekin/mml-hackatune-26/harvester/corpus/audio/; }
    location / { proxy_pass http://127.0.0.1:8001; }
}
```

### Improve Chat recall
The current filter uses only `model.tags: {$in: [...]}`. Adding a semantic
fallback using the full learned-weight score (dot product against query embedding)
would catch tracks that match the vibe without having the exact tags.

---

## File map

```
mml-hackatune-26/
├── pyserver.env              ← env template (this repo, safe to commit)
├── WHAT_IT_IS.md             ← architecture overview
├── HOWTO.md                  ← local dev + deployment
├── HANDOVER.md               ← this file
│
├── data/
│   ├── learned_weights.json  ← 86-dim weights + metadata (server only)
│   ├── idf_cache.json        ← IDF weights (server only)
│   ├── tracks.csv            ← fallback if DB unreachable
│   └── users.csv             ← 462 taste profiles
│
├── frontend/
│   ├── src/
│   │   ├── App.tsx           ← nav + tab routing
│   │   ├── api.ts            ← all API calls + TypeScript interfaces
│   │   ├── pages/
│   │   │   ├── ChatPage.tsx
│   │   │   ├── TastePage.tsx
│   │   │   ├── SimilarPage.tsx
│   │   │   ├── MoodBoardPage.tsx
│   │   │   └── WeightsPage.tsx
│   │   └── components/
│   │       ├── TrackCard.tsx
│   │       ├── TrackModal.tsx
│   │       ├── FeatureMap.tsx    ← Russell circumplex mood map
│   │       └── ScatterPlot.tsx
│   └── dist/                 ← built frontend, SCP'd to discovery/static/
│
├── backend/ (discovery/ on server)
│   ├── app.py                ← FastAPI app, mounts, router includes
│   ├── data_loader.py        ← loads TRACKS + USERS; audio_url()
│   ├── db_search.py          ← tag-vector cache, scoring, filter search
│   ├── rerank.py             ← 86-dim vector math, taste profile, LOO
│   ├── learned_weights.py    ← loads learned_weights.json at startup
│   ├── idf.py                ← IDF weights (fallback)
│   ├── audio_features.py     ← pgvector KNN queries
│   ├── llm.py                ← Gemini calls (intent parse + vision)
│   ├── chat_routes.py        ← /api/chat
│   ├── taste_routes.py       ← /api/taste, /api/users
│   ├── similar_routes.py     ← /api/similar, /api/similar-file, /api/track/*, /api/tracks/search
│   ├── multimodal_routes.py  ← /api/multimodal
│   ├── weights_routes.py     ← /api/weights
│   └── compute_learned_weights.py  ← offline LOO training script
│
└── harvester/                ← Rust crate (ingestion + query binary)
    ├── src/
    │   ├── main.rs           ← harvest binary: download + ingest
    │   ├── bin/query.rs      ← query binary: similarity CLI
    │   ├── features.rs       ← audio feature extraction (rosa DSP)
    │   ├── db.rs             ← Postgres pool + schema helpers
    │   └── compression.rs    ← NCD via zstd (research, not in prod)
    ├── corpus/audio/         ← 50 k MP3s (not in git, server only)
    └── target/release/query  ← compiled binary (not in git, server only)
```

---

## Secrets inventory

| Secret | Where stored | Used by |
|---|---|---|
| `DATABASE_URL` | `discovery/.env` on server | All backend modules, Rust query binary |
| `GEMINI_API_KEY` | `discovery/.env` on server | `llm.py` (Chat + MoodBoard) |
| `CYANITE_API_KEY` | `discovery/.env` on server | `compute_learned_weights.py` only (offline) |
| `CYANITE_ACCOUNT` | `discovery/.env` on server | Same |

None of these are in git. The `pyserver.env` file in the repo is a template with blank values.

---

## Rust vs Python — cross-check findings

*Produced by independent agent analysis of `harvester/src/` and `discovery/`.*

### What each system does

| Capability | Rust `harvester` / `query` | Python `discovery/` |
|---|---|---|
| Audio feature extraction | ✓ Offline: mel (3 res), chroma (3 res + CQT), Tonnetz, MFCC, spectral, RMS, ZCR, rhythm, pitch, aubio extras, NCD sigs | ✗ None at runtime |
| Acoustic KNN (ID→ID) | ✓ 4-axis weighted cosine (`vec_mel+chroma+tonnetz+mfcc`), full table scan | ✓ mel-only HNSW (`vec_mel`), sub-ms via pgvector index |
| Acoustic KNN (file→catalog) | ✓ Extract + 4-axis KNN, ~3–15 s per upload | ✗ Delegates 100% to Rust subprocess |
| NCD reranking | ✓ zstd/chroma and FLAC/PCM, fully implemented | ✗ Not implemented |
| Tag-based semantic search | ✗ | ✓ 86-dim vector, SQL HAVING, scored by learned weights |
| Taste profile / LOO weights | ✗ | ✓ Full system |
| Valence / arousal scoring | ✗ | ✓ From Cyanite ValenceArousalV2 |
| Beta-Bernoulli tag prediction | ✗ | ✓ In `_predict_tags_from_hits` |

### Subprocess interface (Rust → Python)

`/api/similar-file` calls the Rust binary synchronously:

```python
subprocess.run(
    [_QUERY_BIN, "similar-file", tmp_path, "--k", str(limit + 5)],
    capture_output=True, text=True, timeout=30,
    env={**os.environ, "DATABASE_URL": _DB_URL},
)
```

Stdout is a JSON array: `[{"cyanite_id": "abc", "dist": 0.312}, ...]`

`dist` is a weighted sum of 4 pgvector cosine distances with default weights mel=1, chroma=1, tonnetz=0.5, mfcc=1.  
Theoretical range: 0 (identical) → 7.0 (completely opposite). **Fixed** in `similar_routes.py`: `finalScore = max(0, 1 − dist/7.0)`.

The `--w-mel / --w-chroma / --w-tonnetz / --w-mfcc` flags are not passed — Rust defaults apply. Timeout is 30 s with no retry.

### Key asymmetries to know about

**`/similar` (by name) vs `/similar-file` use different acoustic backends:**
- By name → mel-only HNSW (Python, fast, index-backed, single axis)
- By file → 4-axis weighted scan (Rust, slower, more accurate harmonically)

The same seed track searched both ways will return different orderings.

**NCD is fully built but never called at runtime.** The `compression` table (`csize`, `fsize`) is populated for every track during ingestion and stored in Postgres. `query similar --metric ncd` works on the CLI. The webapp never uses it.

**Aubio extras are orphaned.** `spectral_aubio` and `notes` tables are populated but nothing reads them for ranking or similarity. They're surfaced in `/api/track/{cid}` detail only.

### Pros and cons

| | Rust approach | Python approach |
|---|---|---|
| Latency — file query | 3–15 s (full DSP) | N/A |
| Latency — ID KNN | 10–50 ms (full scan, no index) | <1 ms (HNSW index) |
| Acoustic accuracy | High — 4 axes, tunable weights | Lower — mel only |
| Portability | Linux/x86-64, requires ffmpeg | Any OS, pip-installable |
| Maintainability | ~1000 LoC Rust; schema changes break both sides | ~150 LoC Python; easy to extend |
| Semantic richness | None | Full Cyanite tag space (mood, genre, instruments, V/A) |
| NCD support | Yes | No |

### Recommended follow-ups

1. **Fix 4-axis KNN for `/similar` (by name):** Replicate the Rust weighted SQL directly in `audio_features.py` — it's a single `psycopg2` call, no Rust needed. Tonnetz + chroma axes improve harmonic precision noticeably.

2. **Wire NCD reranking into `/similar-file`:** The data is already in Postgres. After the KNN shortlist, one extra query on the `compression` table and a Python-side NCD sort would improve structural-similarity precision at near-zero cost.

3. **Replace subprocess with an axum sidecar:** A small HTTP endpoint on the Rust side would allow streaming progress, remove the 30 s hard timeout, and avoid the temp-file round-trip. `src/bin/gateway.rs` is already an axum skeleton to crib from.

4. **Drop or use the aubio extras:** Either add onset rate / note density as a new similarity axis, or stop computing them to save ingestion time (~15 % of per-track DSP budget).
