# HACKATUNE — Postgres Database Schema (the gory details)

> Live DB: `harvest` on `95.216.72.161` (Postgres 16 + `pgvector` + `vector`/HNSW).
> Verified against the live database on **2026-06-27**. 35 tables, ~57 GB total (the
> `spectrogram` + `compression` blobs dominate). Access read-only with
> `ssh … ekin@95.216.72.161` → `sudo -u postgres psql harvest`.

## 0. Overview

Everything in this project hangs off one universal join key: **`cyanite_id`** — a
Cyanite library-track id of the form `libtr_01KV…` (ULID-ish). The catalog is **50,000
Jamendo tracks** (`tracks`), of which **49,998 are fully audio-analysed** and **10,000
carry Cyanite AI tags**. Three independent description layers are keyed on `cyanite_id`:

1. **Audio feature store** (Rust harvester) — DSP features + a 382-d embedding split
   across four `pgvector` tables, plus raw spectrograms and compression signatures.
2. **Cyanite semantic layer** (Python `cyanite_fetch.py` via the caching gateway) —
   AI mood/genre/instrument/key/era tags and per-segment emotion trajectories.
3. **Jamendo metadata** (free scrape) — human genre/instrument tags + album/artist.

Derived analytics (`lab_result`, `track_xy`, `user_topology`) sit on top, and the
gateway’s own bookkeeping (`cyanite_cache`, `cyanite_quota`) plus the Cyanite knowledge
base (`cyanite_model`, `ontology_term`, `doc`) sit alongside.

### Relationship sketch (everything keys on `cyanite_id`)

```
                         tracks (50,000)  ── jamendo_id ──►  jamendo_track (48,667)
                         cyanite_id PK                       └─ jamendo_tag (184,984: genre/instrument/vartag)
                              │
         ┌────────────────────┼───────────────────────────────────────────────┐
         │  AUDIO (Rust)       │  CYANITE (Python via gateway)                   │  DERIVED
         ▼                     ▼                                                 ▼
  vec_mel(288)          cyanite_track (10,000 scalars: bpm/key/valence/…)   track_xy (49,998 PCA-2D)
  vec_chroma(48)        cyanite_tag  (416,782 rows · 10 multi-label models)  track_xy_basis (the PCA basis)
  vec_mfcc(40)          cyanite_arousal_traj (10,676 VA segment series)      user_topology (462 listeners)
  vec_tonnetz(6)               ▲                                            lab_result (28 cached experiments)
  rhythm/pitch/spectral/       │ registry/KB
  spectral_aubio/notes/rms/    cyanite_model (23) · ontology_term (683) · doc
  zcr/mfcc/tonnetz/chroma_cqt5
  mel_stack/chroma_stack (multi-res)        GATEWAY bookkeeping (no cyanite_id):
  compression (NCD sig+pcm, 14 GB)            cyanite_cache (12,871) · cyanite_quota (3 actions)
  spectrogram (full log-mel, 43 GB)
  melody_cens (WhisperHum hum index)
```

The 382-d audio embedding = **mel(288) ⊕ chroma(48) ⊕ mfcc(40) ⊕ tonnetz(6)**, stored as
four separate `vector` columns each with its own HNSW cosine index, so similarity can be
re-weighted per block (and ablated by dropping a block — see `/api/ablation`).

---

# Section A — Catalog & users

## `tracks` — the master catalog (50,000 rows, 17 MB)
**PURPOSE.** One row per Jamendo track we ingested; the spine of the whole system and
the source of the `cyanite_id ↔ jamendo_id` mapping, display name/artist, and the
ingestion state machine.
**MECHANISM.** Seeded from `data/tracks.csv` (the 50k Jamendo sample uploaded to the
Cyanite library, which minted the `libtr_…` ids). The Rust harvester then claims rows
(`feature_status pending→processing→done`, `SKIP LOCKED` work queue) and fills audio
columns; `download_status` tracks the mp3 fetch.

| column | type | meaning |
|---|---|---|
| `cyanite_id` | text **PK** | Cyanite library id `libtr_…` (universal join key) |
| `jamendo_id` | text NOT NULL | Jamendo track id (also the mp3 filename `<jid>.mp3` and `/audio/{jid}`) |
| `name`, `artist` | text | display metadata (from CSV) |
| `album_block` | text | album grouping id (used by the hierarchy lab; = a jamendo_id of the album’s first track) |
| `duration_csv` | float8 | duration from the CSV |
| `license` | text | Creative-Commons license URL |
| `mp3_path`, `mp3_bytes`, `sha256` | text/int8/text | local audio file bookkeeping |
| `download_status` | text | mp3 fetch state (all `done`) |
| `feature_status` | text NOT NULL `'pending'` | analysis state: **49,998 `done`, 2 `failed`** |
| `worker_id`, `claimed_at` | text/timestamptz | distributed-worker lease (SKIP LOCKED queue) |
| `duration_audio` | real | duration measured from the decoded audio |
| `error` | text | last failure reason |
| `updated_at` | timestamptz | last state change |
| `fts` | tsvector | GIN full-text over name+artist (powers `/api/fts`, `/api/cyanite_search`) |

**INDEXES.** `tracks_pkey(cyanite_id)`; `idx_tracks_artist(artist)` (artist browse);
`idx_tracks_fstatus(feature_status)` (the worker queue scan); `tracks_fts_idx` GIN(fts)
(catalog text search).
**RELATIONSHIPS.** `cyanite_id` → every audio/cyanite table; `jamendo_id` → `jamendo_track`.

## Users
There is **no `users` table** — listeners + their likes are loaded from
`data/users.csv` into memory by `webapp/server.py` (`USERS` dict: user_id → list of
liked jamendo_ids). 462 users (matching `user_topology`). Likes are resolved to
`cyanite_id` via the in-memory `JID2CID` map at request time.

---

# Section B — Audio feature store / embeddings

All written by the **Rust harvester** (`harvester/src/`, binaries `harvest` +
`spectrogram`). One row per analysed track, keyed `cyanite_id`. Coverage ≈ 47.7k–49.998k
(small differences = a handful of tracks that failed an individual extractor). `bytea`
columns are little-endian `f32`/`f64` arrays unless noted.

## The 382-d embedding — `vec_mel` / `vec_chroma` / `vec_mfcc` / `vec_tonnetz`
**PURPOSE.** The acoustic fingerprint used for *all* similarity, clustering, steering,
ICA, ablation and manifold work. Four blocks kept separate so each can be weighted or
hidden independently.
**MECHANISM.** Harvester computes summary statistics per block and writes a single
`pgvector` `vector` per table. Verified dims: **mel 288, chroma 48, mfcc 40, tonnetz 6**
(= 382 concatenated). `webapp/server.py:track_embeddings()` joins all four and concatenates
in this order; `EMB_BLOCKS = [(mel,0,288),(chroma,288,336),(mfcc,336,376),(tonnetz,376,382)]`.

| table | column | dim | rows | size | meaning |
|---|---|---|---|---|---|
| `vec_mel` | `embedding vector` | 288 | 49,998 | 147 MB | mel-band timbre/texture summary (64+96+128 multi-res) |
| `vec_chroma` | `embedding vector` | 48 | 48,132 | 40 MB | harmony/key (12 bins × {mean,std,median,mad}) |
| `vec_mfcc` | `embedding vector` | 40 | 48,136 | 37 MB | timbre detail (20 MFCCs × {mean,std}) |
| `vec_tonnetz` | `embedding vector` | 6 | 47,778 | 23 MB | tonal-centroid (6-d Tonnetz) |

**INDEXES.** Each has `*_pkey(cyanite_id)` + **`*_hnsw` HNSW index on `embedding` with
`vector_cosine_ops`** — the approximate-NN index behind `/api/similar` (Postgres computes
the weighted cosine `w_mel·(m<=>q)+…` directly in SQL).

## Per-track DSP statistics (scalar tables)
**PURPOSE.** Interpretable, human-named audio descriptors used for explanations, the
7-axis taste profile, and lab features. **MECHANISM.** Harvester `extract()` per track.

| table | rows | key columns | what it is |
|---|---|---|---|
| `rhythm` | 47,771 | `tempo_bpm, beat_bpm, beat_count, beat_regularity, tempo_confidence, onset_rate` | tempo/beat (aubio + DSP) |
| `pitch` | 47,776 | `median_hz, voiced_frac, voiced_conf, tuning` | pitch / vocalness / tuning offset |
| `spectral` | 47,766 | centroid/rolloff/bandwidth/flatness/flux × {mean,std,median,mad} | spectral shape (20 cols) |
| `spectral_aubio` | 46,575 | skewness/kurtosis/slope/decrease × {mean,std} | higher-order spectral moments (aubio) |
| `notes` | 46,978 | `note_count, note_mean_dur` | note-onset stats |
| `rms` | 47,768 | `mean,std,median,mad` | loudness/energy envelope |
| `zcr` | 47,770 | `mean,std,median,mad` | zero-crossing rate (noisiness proxy) |
| `mfcc` | 48,126 | `mean,std,median,mad` (bytea, 20-d each) | raw MFCC stat vectors (feeds `vec_mfcc`) |
| `tonnetz` | 47,764 | `mean,std,median,mad` (bytea, 6-d) | raw Tonnetz stats (feeds `vec_tonnetz`) |
| `chroma_cqt5` | 47,761 | `mean,std,median,mad` (bytea) | CQT-5-octave chroma stats (feeds `vec_chroma`) |

All keyed `cyanite_id` (PK), each a plain btree.

## Multi-resolution stacks — `mel_stack` / `chroma_stack` (143k rows each, 274/53 MB)
**PURPOSE.** Multi-window-length spectral summaries (3 levels per track → ~143k =
~3×48k). Richer than the single `vec_*` summary; an intermediate/experimental store.
**MECHANISM.** Harvester writes one row per `(cyanite_id, level)`; `win/hop/n_bands/
n_frames` describe the resolution; `mean/std/median/mad` are `bytea` stat vectors.
**INDEXES.** PK `(cyanite_id, level)`. *Largely superseded by the `vec_*` embedding for
serving; retained as raw material.*

## `compression` — NCD similarity signatures (47,778 rows, **14 GB**)
**PURPOSE.** Powers the Normalized Compression Distance metric (a model-free,
information-theoretic similarity completely independent of the embedding) used by
`/api/ncd_cosine` and the Rust `query ncd-pairs`/`similar --metric ncd`.
**MECHANISM.** Harvester stores `sig` = zstd-compressed chroma/feature signature and
`pcm` = a downsampled PCM snippet, with their compressed sizes `csize`/`fsize`. NCD is
computed pairwise at query time from these. Sample: `sig`≈25 KB, `pcm`≈258 KB/row.

| column | type | meaning |
|---|---|---|
| `cyanite_id` | text PK | |
| `sig` | bytea | zstd-compressed feature signature |
| `csize` | int | compressed size of `sig` (the C(x) term for NCD) |
| `pcm` | bytea | downsampled PCM for audio-domain NCD (`ncd-audio`) |
| `fsize` | int | compressed size of the PCM/FLAC stream |

## `spectrogram` — full log-mel spectrograms (49,998 rows, **43 GB**, the biggest table)
**PURPOSE.** The raw visual/auditable input behind every audio feature; served as a PNG
by `/api/spectrogram/{cid}` and shown in the Spectrograms tab.
**MECHANISM.** The Rust `spectrogram` binary (6-worker fleet) computes a 128-mel
spectrogram, power→dB clamped to `[-80,0]`, quantised to **u8 (0.31 dB/step)**, row-major
`[n_mels][n_frames]`, then **zstd level-9** → `data bytea`. Decode: `zstd -d` →
`np.frombuffer(uint8).reshape(n_mels,n_frames)`.

| column | type | meaning |
|---|---|---|
| `cyanite_id` | text PK | |
| `n_mels` | int | 128 |
| `n_frames` | int | ~1673 (≈38.9 s at sr/hop) |
| `sr`,`n_fft`,`hop` | int | 22050 / 2048 / 512 |
| `db_min`,`db_max` | real | -80 / 0 (the u8 quantisation range) |
| `data` | bytea | zstd(u8 matrix), ~170 KB compressed (214,144 B raw) |

## `melody_cens` — WhisperHum query-by-humming index (49,998 rows, 75 MB)
**PURPOSE.** Pre-computed chroma/CENS contour per track so a hummed query can be matched
by DTW without re-decoding the catalog. Backs `/api/hum_search`.
**MECHANISM.** Built by `/api/hum_index_build` from each track’s stored chroma (Rust
`query hum-chroma`/`hum-bytes`). `nframes`≈120 (~1 Hz), `keyidx` = estimated key
(0–11), `cens bytea` = 12×nframes CENS matrix (1440 B sample).

---

# Section C — Cyanite semantic layer

Written by **`harvester/cyanite_fetch.py`**, which calls the per-track models endpoint
`GET /v1/library-tracks/{cyanite_id}/models?model=…` **through the caching gateway** (one
quota’d call per track, *not* search). Coverage: **10,000 tracks** (the tagging cap was
hit at exactly 10k; quota used 12,838 / 50,000 because some tracks needed re-fetches).

## `cyanite_track` — flattened scalar attributes (10,000 rows, 5.7 MB)
**PURPOSE.** The single-valued Cyanite outputs (one per track) lifted into columns for
cheap filtering, the explainer, and the clickable attribute distributions.
**MECHANISM.** `cyanite_fetch.py` parses the model envelope and writes the scalar fields.
**Note:** the clickable “Musical key / Energy / Emotion / Era” bars read THESE columns,
not `cyanite_tag` — that is why `/api/by_tag` must fall back to scalar-column matching.

| column | type | source model | meaning |
|---|---|---|---|
| `cyanite_id` | text PK | | |
| `bpm` | int | BpmV2 | tempo (60–200) |
| `key` | text | KeyV2 | musical key, e.g. `aMinor`, `cMajor` |
| `time_signature` | text | TimeSignatureV2 | e.g. `4/4` (mostly empty in data) |
| `valence` | real | ValenceArousalV2 | −1..1 pleasantness |
| `arousal` | real | ValenceArousalV2 | −1..1 energy/activation |
| `energy_level` | text | ValenceArousalV2 | categorical (low/varying/high…) |
| `emotion_profile` | text | ValenceArousalV2 | categorical emotion label |
| `era` | int | MusicalEraV2 | estimated production year (e.g. 2008) |
| `vocal_presence` | text | VocalsV2 | low/medium/high (often empty) |
| `vocal_gender` | text | VocalsV2 | male/female (often empty) |
| `description` | text | AutoDescriptionV2 | free-text track description |
| `fetched_at` | timestamptz | | |
| `fts` | tsvector | | GIN over the description (search) |

**INDEXES.** `cyanite_track_pkey`, `cyanite_track_fts_idx` GIN(fts).

## `cyanite_tag` — multi-label AI tags (416,782 rows, 71 MB) ⭐
**PURPOSE.** The many-tags-per-track outputs: mood, genre, subgenre, character,
movement, instruments, vocals, “music-for” use-cases, free-genre. The semantic engine of
the whole recommender (steering, `/api/by_tag`, grounding, calibration).
**MECHANISM.** `cyanite_fetch.py` writes one row per `(cyanite_id, model, tag)` with the
model’s confidence in `score`. **PK `(cyanite_id, model, tag)`.**

| column | type | meaning |
|---|---|---|
| `cyanite_id` | text | track |
| `model` | text | which AI model emitted the tag |
| `tag` | text | the label (e.g. `ambient`, `piano`, `ethereal`) |
| `score` | real **nullable** | **per-tag confidence 0–1 — NULL for unscored models** |

### Model breakdown (verified live)
| model | rows | tracks | score? | score min/avg/max |
|---|---|---|---|---|
| MusicForV1 | 190,001 | 10,000 | **NULL** (dominant tags only) | — |
| MoodAdvancedV2 | 51,272 | 9,999 | ✅ | 0.121 / 0.393 / 0.875 |
| InstrumentsV2 | 41,246 | 9,977 | **NULL** (presence categorical, scores dropped) | — |
| FreeGenreV3 | 36,254 | 9,999 | **NULL** (free text, no scores) | — |
| MoodSimpleV2 | 21,942 | 9,999 | ✅ | 0.202 / 0.539 / 0.990 |
| CharacterV2 | 21,139 | 10,000 | ✅ | 0.040 / 0.389 / 0.979 |
| MovementV2 | 17,850 | 9,999 | ✅ | 0.059 / 0.417 / 0.981 |
| MainGenreV2 | 15,884 | 9,999 | ✅ | 0.028 / 0.361 / 0.997 |
| VocalsV2 | 11,589 | 10,000 | ✅ | 0.038 / 0.467 / 1.000 |
| SubgenreV2 | 9,605 | 5,048 | ✅ | 0.083 / 0.479 / 0.962 |

## `cyanite_arousal_traj` — within-track emotion trajectories (10,676 rows, 5.7 MB)
**PURPOSE.** The per-segment valence/arousal time-series (model `ValenceArousalV2`), used
by `/api/trajectories` to cluster emotional “journeys.”
**MECHANISM.** `harvester/cyanite_segments_fetch.py` parses the `segments` block of the
ValenceArousalV2 model output. **Built almost entirely from already-cached gateway
responses → ~zero extra quota.** PK `(cyanite_id, model)`.

| column | type | meaning |
|---|---|---|
| `cyanite_id` | text | |
| `model` | text | `ValenceArousalV2` |
| `ts` | real[] | segment timestamps (seconds); array len varies (12–25 in samples) |
| `arousal` | real[] | per-segment arousal, aligned to `ts` |
| `valence` | real[] | per-segment valence, aligned to `ts` |

## `cyanite_model` — the 23-model knowledge base (23 rows, registry)
**PURPOSE.** Documents every Cyanite AI model: kind, vocabulary, value range, and the
shape of its segment time-series. Powers `/kb` and the schema-aware UI/explanations.
**MECHANISM.** Loaded once from the Cyanite docs into Postgres (`kind`, `vocabulary`,
`fields jsonb`, `value_range`, `segment_shape`, `description`, `example`). PK `name`.

---

# Section D — Gateway cache/quota + Cyanite KB

## `cyanite_cache` — the caching reverse-proxy store (12,871 rows, 290 MB)
**PURPOSE.** Every upstream Cyanite REST response is cached here so repeats cost **zero**
pooled quota. The reason we can “hit the real API” safely. **MECHANISM.** The Rust
`gateway` binary (systemd `slu-gateway`, :8080) keys each response by a canonical
`cache_key` (method+path+query+body), stores the raw `body`, and increments `hits`.

| column | type | meaning |
|---|---|---|
| `cache_key` | text PK | canonical request hash |
| `method`,`path`,`query` | text | the proxied request |
| `action` | text | `tagging` / `prompt_search` / `similarity` (quota bucket) |
| `status` | int | upstream HTTP status (only 2xx cached; 429/5xx refund quota) |
| `content_type`,`body` | text | the cached response |
| `hits` | int8 | cache-hit counter |
| `created_at`,`last_hit` | timestamptz | |

By action: **tagging 12,834** (the per-track model fetches), **prompt_search 60**,
**similarity 20**.

## `cyanite_quota` — pooled quota debit ledger (3 rows)
**PURPOSE.** Tracks consumption against the shared Cyanite caps so the gateway can reject
locally before wasting an upstream unit. **MECHANISM.** Gateway debits on cache-miss,
refunds on upstream 429/5xx.

| action | used | cap |
|---|---|---|
| `prompt_search` | 60 | 15,000 |
| `tagging` | 12,838 | 50,000 |
| `similarity` | 20 | 15,000 |

## `ontology_term` (683) + `doc` — Cyanite knowledge base
`ontology_term(vocabulary, value, idx)` PK `(vocabulary,value)` — the 683 controlled
vocabulary terms across all taxonomy models (the legal tag values). `doc(name, kind,
path, bytes, content)` — full Cyanite documentation pages stored for `/library`. Both
loaded once; serve the KB endpoints and validate tag inputs.

---

# Section E — Lab / derived results

## `lab_result` — the derived-fact cache (28 rows, 1 MB)
**PURPOSE.** Compute-once persistence for every heavy lab endpoint, so expensive sklearn
fits survive restarts and are SQL-queryable. **MECHANISM.** `webapp/server.py:lab_cache()`
keys on `experiment:vN:params`, stores the JSON `payload`, and only recomputes when
`inputs_hash` (a content stamp = count+max-mtime+md5 over the consumed rows) changes.

| column | type | meaning |
|---|---|---|
| `key` | text PK | `experiment:vN:params` (e.g. `ablation:v1:tags:n700`) |
| `payload` | jsonb | the exact endpoint response (NaN/Inf sanitised to null) |
| `inputs_hash` | text | sha256 of the coverage stamp; mismatch ⇒ recompute |
| `schema_ver` | int | bumped to invalidate a whole experiment family |
| `n_inputs` | int | primary row count (coverage at-a-glance) |
| `computed_at` | timestamptz | |

**INDEXES.** PK(key) + `lab_result_payload_gin` GIN(payload jsonb_path_ops) for
attribute queries. Live keys include `ica:*`, `ablation:*`, `manifold:v2:*:pacmap:*`,
`tag_classifiers:*`, `trajectories:*`, `mmd_rbf_perm:*`, `n2_grounding:*`,
`calibration:*`, `ncd_cosine_mantel:*`, `qrewrite:*` (cached Claude search rewrites).

## `track_xy` (49,998) + `track_xy_basis` (1) — global 2-D projection
**PURPOSE.** A fixed PCA-2D layout of the whole catalog for the Map/Discover scatter, so
every track has stable `(x,y)` coordinates. **MECHANISM.** Fleet-computed PCA basis
(`track_xy_basis`: `method=pca2_l2`, `mean`+`comp` bytea) projects each track’s L2-normed
embedding into `track_xy(x,y,method)`.

## `user_topology` — per-listener TDA (462 rows)
**PURPOSE.** Persistent-homology summary of each listener’s taste cloud (the Topology
lab). **MECHANISM.** `n_points/n_landmarks/n_stable_clusters/band/var_retained` + the H0
barcode in `h0_bars jsonb`. PK `user_id`.

## `failures` (4 rows)
Harvester error log (`cyanite_id, stage, reason, ts`). The 4 rows are transient DB
disconnects during ingestion — not data-quality issues (49,998/50,000 completed).

---

# Cyanite tag reliability & confidence: what we store (and what we DON'T)

This is the crux of “how much do we trust the Cyanite tags.” Precise, verified status:

### What `score` means and where it exists
`cyanite_tag.score` is **Cyanite’s own per-tag confidence/probability** for the
taxonomy-classifier models — a 0–1 number (higher = more confident the tag applies).
It **is stored** for the 7 scored models: **MoodAdvancedV2, MoodSimpleV2, CharacterV2,
MovementV2, MainGenreV2, VocalsV2, SubgenreV2** (avg scores ~0.36–0.54; full 0–1 spread).

### Where confidence is MISSING (stored as NULL by design — Cyanite returns none)
- **MusicForV1, FreeGenreV3** — “dominant tags only / free text, no scores.” We record
  the tag’s *presence* but there is **no confidence** (190,001 + 36,254 NULL rows).
- **InstrumentsV2** — emits a categorical *presence* level (absent/partially/frequently/
  throughout) per instrument, not a 0–1 score; we store the instrument tags with
  **NULL score** (41,246 rows). The presence category is currently **dropped**.

### Where confidence is DROPPED in our flattening (a real gap) ⚠
The musical-parameter models **KeyV2, BpmV2, TimeSignatureV2** each return a full
`Confidence` struct in the API — `model_certainty`, `prediction_stability`, `confidence`
(all 0–1). We flatten only the *value* into `cyanite_track.key / bpm / time_signature`
and **do NOT store the confidence struct**. So today you cannot ask “how sure is Cyanite
that this is aMinor?” — that information was discarded at ingest. (Same for
`MusicalEraV2.estimatedProductionYear` → `era`, which has no confidence anyway.)
**ValenceArousalV2** values are stored (`valence/arousal/energy_level/emotion_profile` +
the `cyanite_arousal_traj` time-series) but its outputs are point estimates, no struct.

### Models in the registry but NOT ingested at all (no rows anywhere)
`AiMusicDetectionV1, AudioFileInfoV1, AugmentedKeywordsV3, RepresentativeSegmentV2,
TempoV1, VocalStyleV1, VoiceoverV2` — defined in `cyanite_model` but never fetched, so we
have no data for them. (AutoDescriptionV2 *is* ingested → `cyanite_track.description`.)

### How we EXTERNALLY validate that reliability (don’t just trust Cyanite)
Because the stored `score` is Cyanite’s *self-reported* confidence, we cross-check it
against independent ground truth:
- **`/api/calibration`** — reliability diagram + **ECE** of each scored tag against an
  *independent DSP proxy* (e.g. does “energetic ≥ 0.x” actually track RMS-energy?). It is
  framed as a **one-sided lower bound** on quality (proxy error only widens the gap), with
  a calibrated-null ECE band and held-out evaluation — never “the score is a probability.”
- **`/api/grounding`** — per-tag **cross-validated R²** of predicting each Cyanite tag
  from the raw 382-d audio embedding (artist-grouped CV, present-only, permutation null).
  High R² ⇒ the tag is acoustically grounded; low R² ⇒ it encodes something audio alone
  can’t recover (or is noisy).
- **`/api/tag_classifiers` / `/api/predict_tags`** — our own logistic-regression bank
  (CV-AUC) over the embedding, both an independent reliability read on each tag *and* the
  mechanism that predicts tags for uploaded audio.

**Bottom line:** we store Cyanite’s confidence for the 7 scored taxonomy models; we have
*no* confidence for the unscored/presence/free models; and we **dropped** the
key/bpm/time-signature confidence structs — a worthwhile future ingest fix if per-key
trust matters.

---

## Surprises / notes for maintainers
- **`spectrogram` (43 GB) + `compression` (14 GB)** are ~99% of the DB on disk; everything
  else is < 1 GB combined.
- **`mel_stack` / `chroma_stack`** (143k rows each) are multi-resolution raw material now
  largely superseded by the `vec_*` embedding for serving — candidates for archival.
- **`spectral_aubio`, `zcr`, `notes`, `chroma_cqt5`** feed the embedding/explanations but
  are not all surfaced in the UI.
- **`time_signature`, `vocal_presence`, `vocal_gender`** in `cyanite_track` are frequently
  empty (Cyanite returned null for many tracks).
- The catalog is `libtr_…`-keyed but the **Cyanite search library is larger than our 50k**
  — search hits can return ids not in `tracks` (they still play via the title’s jamendo_id).
- `cyanite_tag` covers **only 10 of 23** models; the rest are either flattened into
  `cyanite_track` scalars or never ingested (see reliability section).
