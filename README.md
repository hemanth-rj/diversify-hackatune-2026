# 🎵 Diversify — Contextual Discovery

**Explainable, audio-based music discovery on the Cyanite API.**
Built for the Cyanite challenge at HACKATUNE 2026 (Munich Music Labs).

Find the right track through *how it actually sounds* — by brief, conversation, a listener's
taste, a reference song, or an image — and every result can answer **"why this track?"** grounded
in Cyanite's audio analysis.

## The five ways in

| Tab | What it does |
|---|---|
| **Brief** | Type a scene/brief → a persona-aware pitch list, each track with a reason |
| **Chat** | Conversational, steerable discovery |
| **Taste** | Pick a listener → their taste profile + a personalized stream |
| **Similar** | Upload a reference audio file → acoustically similar tracks |
| **MoodBoard** | Upload an image → music that matches its visual vibe |

Every track opens a detail view with its Cyanite tags, audio features, and a **log-mel
spectrogram**.

## Repository layout

```
frontend/            React + Vite + TypeScript app (the UI)
backend/             Brief/Taste FastAPI — the backend you run locally
discovery-service/   Discovery engine source (Chat/Similar/MoodBoard) — runs as a hosted service
docs/                Architecture, decisions, deployment
docker-compose.yml   One-command local run (frontend + backend)
```

> **Heads-up on what runs where.** `frontend/` + `backend/` run on any machine (below). The
> **discovery** and **deep** services behind Chat/Similar/MoodBoard/spectrograms need a 57 GB
> Postgres DB + a Rust feature-extractor + a 50k-track audio corpus, so they can't run on a
> laptop — the app consumes them as **hosted services** over the network (configurable via env).
> Out of the box those point at the team's hosted instance; **Brief and Taste are fully local.**

## Run it (clone-and-run)

### Prereqs
Python 3.11+, Node 20+, and Cyanite + Gemini API keys.

### 1. Backend (Brief/Taste) — `backend/`

```bash
cd backend
cp .env.sample .env          # then fill in your keys (see below)
pip install -r api/requirements.txt
python api/warm_cache.py      # optional: pre-bake the 5 listeners for instant Taste
uvicorn api.server:app --port 8000
```

`backend/.env`:

```
CYANITE_API_KEY=cyk__...
CYANITE_ACCOUNT=acc_...
GEMINI_API_KEY=AIza...
JAMENDO_CLIENT_ID=...          # optional, for artwork
# CYANITE_BASE_URL=...         # optional, route Cyanite through a caching gateway
```

### 2. Frontend — `frontend/`

```bash
cd frontend
npm install
npm run dev                    # http://localhost:5173
```

Backend URLs are configurable env vars (sensible defaults if unset):

| Var | Points at | Default |
|---|---|---|
| `VITE_DIVERSIFY_API` | the local Brief/Taste backend | `http://localhost:8000/api` |
| `VITE_ORKUN_API` | hosted discovery service | `http://95.216.72.161:8001/api` |
| `VITE_ERKIN_API` | hosted deep service (spectrograms) | `http://95.216.72.161:8000` |

With the backend on `:8000` and the frontend on `:5173`, **Brief and Taste work fully locally**;
Chat/Similar/MoodBoard use the hosted services.

### Or: one command with Docker

```bash
cp backend/.env.sample backend/.env   # add your keys
docker compose up --build             # frontend on http://localhost:5173, backend on :8000
```

## Tests & CI

`cd frontend && npm run build` runs `tsc` (full typecheck) then `vite build` — a green build is
the type + compile gate, and CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs it on
every push/PR. The backend has its own `pytest` + `ruff` suite (`cd backend && pytest`).

## Architecture & decisions

Hybrid: **Cyanite is the recommendation engine; a self-built audio layer validates and extends it
but never replaces it**, and **no collaborative filtering** is shipped. Full detail in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), rationale in [docs/DECISIONS.md](docs/DECISIONS.md),
hosted deployment in [docs/DEPLOY.md](docs/DEPLOY.md).
