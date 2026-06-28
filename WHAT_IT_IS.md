# Sounds Like You — What It Is

Music discovery web app built for HACKATUNE 2026 (Munich Music Labs / Cyanite challenge).
Live at **http://95.216.72.161:8001**

---

## In one sentence

Given a user's taste, a text description, a seed track, or an image, find the most
acoustically relevant tracks from a 50 k-track Jamendo catalog — and explain why each
one matches.

---

## The catalog

| Fact | Value |
|---|---|
| Tracks | 50,000 Jamendo MP3s |
| With Cyanite AI tags | ~10,000 (the subset that has been run through Cyanite) |
| With audio feature vectors | ~10,000 (mel, chroma, MFCC, Tonnetz — extracted by Rust harvester) |
| Users with taste profiles | 462 (pseudonymized, from the challenge data pack) |
| Avg liked tracks per user | ~31 |

Every track is represented as an **86-dimensional tag vector**:

| Dims | Source | What they capture |
|---|---|---|
| 0–12 (13) | Cyanite MoodSimpleV2 | chill, dark, energetic, happy, … |
| 13–35 (23) | Cyanite MainGenreV2 | ambient, electronic, jazz, rock, … |
| 36–82 (47) | Cyanite InstrumentsV2 | percussion, synth, piano, guitar, … |
| 83 | Cyanite ValenceArousalV2 | valence (−1 → +1, mapped to 0 → 1) |
| 84 | Cyanite ValenceArousalV2 | arousal (−1 → +1, mapped to 0 → 1) |
| 85 | Cyanite BpmV2 | BPM normalised to 0 → 1 (cap 300) |

In addition, each track has a **382-dim raw audio embedding** (concatenation of
mel/chroma/MFCC/Tonnetz vectors) stored in pgvector for acoustic KNN.

---

## The five tabs

### Chat
Type a natural-language vibe description. Gemini 2.5 Flash parses it into a
structured filter (`{model}.tags: {$in: [...]}`) and a free-text summary.
`db_search.search_by_filter()` runs a tagged SQL query with AND-across-models
semantics, scores the results against **learned weights**, and returns up to 10 tracks.
Fallback chain: strict AND → loose OR → full catalog scored by learned weights
(always returns something).

Results show in a grid or a **Russell circumplex mood map** (valence × arousal scatter).

### Taste
Pick one of 462 pseudonymous users. Their liked tracks are loaded, an 86-dim
taste profile is computed (mean vector × inverse-variance × IDF), and the full
10 k-track tagged catalog is scored by dot product against that profile. Returns
top 20. A seed fingerprint shows which dimensions drive the recommendations.

### Similar
**By track name**: search the catalog by name, pick a seed. Seed's tag vector is
used as weights; catalog is scored. Acoustic similarity from pgvector HNSW index
is blended in (60 % tag / 40 % audio). Grid or scatter view.

**By audio file**: upload any MP3/WAV/FLAC. The Rust `query similar-file` binary
extracts a 382-dim embedding and runs a pgvector KNN search. Results come back
ranked by acoustic distance. A Beta-Bernoulli tag transfer ("predicted audio
profile") estimates the uploaded track's Cyanite tags from its acoustic neighbors.

### MoodBoard
Upload an image (or type a brief). Gemini Vision describes the image as a music
brief, then the same filter → score pipeline as Chat runs. Shows a mood map.

### Insights
Visualises the **learned weights** (produced offline by `compute_learned_weights.py`).
86-dim bar chart, category donut (Instruments 55 %, Valence/Arousal 28 %, Mood 10 %,
Genre 5 %, BPM 2 %), and a plain-English explanation of the LOO training process.

---

## How recommendations work

```
User query / taste profile / seed track
         │
         ▼
  86-dim weight vector
  (learned weights  ──×──  IDF weights  ──×──  user variance)
         │
         ▼
  search_by_filter()  or  score_catalog()
  ├── SQL: WHERE tag IN (...) GROUP BY track HAVING (AND across models)
  └── Numpy dot product of weight vector against all cached tag vectors
         │
         ▼
  Blend with pgvector acoustic KNN (where available)
  finalScore = 0.6 × tagSim + 0.4 × audioSim
         │
         ▼
  Ranked results with match_breakdown (top 3 agreeing dimensions)
```

### Learned weights

Offline LOO process (14,476 folds across 462 users):
- For each user × liked track: hold the track out, compute mean of remaining liked tracks
- `agreement[dim] = seed_mean[dim] × held_out[dim]`  (high only where both are high)
- Average across all folds, multiply by IDF, normalise → `learned_weights.json`

Top dimensions: percussion 18 %, synth 14 %, arousal 14 %, valence 14 %, electronicDrums 8 %

---

## Stack

| Layer | Tech |
|---|---|
| Frontend | React 18, TypeScript, Vite, Recharts |
| Backend | FastAPI, Uvicorn, Python 3.13 |
| Database | PostgreSQL 16 + pgvector |
| Audio features | Rust crate (`harvester`) — mel/chroma/MFCC/Tonnetz via `rosa` |
| Similarity index | pgvector HNSW (`vector_cosine_ops`) |
| LLM | Gemini 2.5 Flash (google-genai SDK) |
| Audio serving | FastAPI StaticFiles → local MP3 corpus (50 k files) |
| Deployment | systemd (`slu-discovery`), single server, no containers |

---

## What the Rust harvester did

The Rust crate (`harvester`) ran once (overnight) to:
1. Download 50 k MP3s from Jamendo (rate-limited, resumable)
2. Extract audio features: mel spectrogram (288-d), chroma (48-d), MFCC (40-d), Tonnetz (6-d)
3. Write features into Postgres tables (`vec_mel`, `vec_chroma`, `vec_mfcc`, `vec_tonnetz`)
4. Build HNSW indexes on those tables for fast pgvector KNN

It also provides the `query` binary (read-only), which the Python webapp calls via
subprocess for the "similar by audio file" feature (extracts the same 382-d embedding
from an uploaded file and runs KNN against the catalog).

The Rust crate is **not running** in production — it ran as a one-time ingestion pipeline.
Only the `query` binary remains in use, called on-demand by the Python backend.
