# 🎵 Diversify — Contextual Discovery

**Explainable, audio-based music discovery on the Cyanite API.**
Built for the Cyanite challenge at HACKATUNE 2026 (Munich Music Labs).

Diversify lets you find the right track through *how it actually sounds* — by brief, by
conversation, by a listener's taste, by a reference song, or by an image — and every result
can answer **"why this track?"** grounded in Cyanite's audio analysis.

## The five ways in

| Tab | What it does | Powered by |
|---|---|---|
| **Brief** | Type a scene/brief → a persona-aware pitch list of sync-ready tracks, each with a reason | Cyanite live (prompt search + tags) |
| **Chat** | Conversational, steerable discovery — describe a vibe and refine it | Cyanite tags + learned-weight ranking |
| **Taste** | Pick a listener → their taste profile (mood / valence / instruments) + a personalized stream | Cyanite multi-track similarity |
| **Similar** | Upload a reference audio file → acoustically similar tracks + predicted audio profile | Raw audio features (pgvector / NCD) |
| **MoodBoard** | Upload an image → music that matches its visual vibe | Gemini Vision → Cyanite |

Every track opens a detail view with its Cyanite tags, audio features, and a **log-mel
spectrogram** — the "why this track?" the brief asks for.

## Architecture (hybrid, three backends behind one UI)

A single React frontend talks to three services. Cyanite is the recommendation engine; the
self-built audio layer validates and extends it (it never replaces Cyanite as the recommender).

```
                 React + Vite + TypeScript frontend (this repo /frontend)
                 Brief · Chat · Taste · Similar · MoodBoard
                          │            │              │
        product path      │   discovery path          │  cover art / spectrogram
                          ▼            ▼               ▼
            Diversify backend     Discovery backend      Deep server
            (FastAPI, Cyanite     (Postgres + learned     (audio features,
             live + personas)      weights + audio)        grounding, spectrograms)
                          │
                  Cyanite via caching gateway (cached, quota-safe)
```

- **Diversify backend** — FastAPI (`mml-hackatune-26`): Brief/Taste/Chat/Image, persona ranking,
  sync-clearance, response caching, Jamendo artwork proxy. Calls Cyanite through a caching
  gateway. (Its own CI/tests/docs live in that repo.)
- **Discovery backend** (`backend/` here) — Orkun's engine: Chat/Similar/MoodBoard over Postgres.
- **Deep server** — Erkin's research service: audio similarity, spectrograms, and audio↔tag
  grounding/validation.

Full detail in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); the merge rationale is in
[docs/DECISIONS.md](docs/DECISIONS.md); deployment in [docs/DEPLOY.md](docs/DEPLOY.md).

## Run it locally

**Frontend:**

```bash
cd frontend
npm install
npm run dev          # http://localhost:5173
```

The frontend reads backend URLs from build-time env vars (sensible localhost/team-server
defaults if unset):

| Var | Points at | Default |
|---|---|---|
| `VITE_DIVERSIFY_API` | Diversify backend `/api` | `http://localhost:8000/api` |
| `VITE_ORKUN_API` | Discovery backend `/api` | `http://95.216.72.161:8001/api` |
| `VITE_ERKIN_API` | Deep server (spectrograms) | `http://95.216.72.161:8000` |
| `VITE_JAMENDO_CLIENT_ID` | (optional) artwork | falls back to the proxy |

The **Diversify backend** must be running for Brief/Taste (`uvicorn api.server:app --port 8000`
from the `mml-hackatune-26` repo); the discovery + deep backends are already live on the team
server.

## Tests & CI

`npm run build` runs `tsc` (full typecheck) then `vite build`, so a green build is the type +
compile gate. CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs it on every
push/PR. The Diversify backend has its own pytest + ruff CI in its repo.

```bash
cd frontend && npm run build   # typecheck + production build
```

## Constraints honored

Content/audio-based and **Cyanite-central**; **no collaborative filtering** (the audio engine
runs CF only as an offline benchmark, never shipped); no raw embeddings used as the recommender.
See [docs/DECISIONS.md](docs/DECISIONS.md).
