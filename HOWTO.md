# Sounds Like You — How To

---

## Run locally (development)

You need Python 3.10+, Node 18+, and access to the server's Postgres.

### 1. Environment

```bash
cp pyserver.env backend/.env
# Edit backend/.env — fill in DATABASE_URL (ask the team for the connection string)
# Add GEMINI_API_KEY if you want Chat and MoodBoard to work
```

### 2. Backend

```bash
cd backend
pip install fastapi "uvicorn[standard]" psycopg2-binary numpy python-dotenv \
            pydantic scipy "google-genai"
uvicorn app:app --reload --port 8001
```

The backend starts and immediately prewarns the tag-vector cache (~10 k vectors) and
valence/arousal map from Postgres. First request may be slow while that completes;
watch the console for `[db_search] prewarm complete`.

### 3. Frontend

```bash
cd frontend
npm install
npm run dev          # Vite dev server on :5173, proxies /api → :8001
```

Open http://localhost:5173.

### What works locally

| Tab | Works? | Requirement beyond DATABASE_URL |
|---|---|---|
| Taste | ✓ | — |
| Similar (by name) | ✓ | — |
| Insights | ✓ | `data/learned_weights.json` (falls back to IDF if absent) |
| Chat | ✓ | `GEMINI_API_KEY` |
| MoodBoard | ✓ | `GEMINI_API_KEY` |
| Audio playback | ✓ (CDN) | Falls back to Jamendo CDN automatically |
| Similar (by audio file) | ✗ | Rust binary compiled for Linux/x86-64 only |

---

## Deploy to the server

**Never use git push for deployment. Use SCP only.**
Secrets live exclusively in `/home/ekin/mml-hackatune-26/discovery/.env` on the server.

### Frontend

```bash
cd frontend
npm run build
scp -r dist/* ekin@95.216.72.161:~/mml-hackatune-26/discovery/static/
```

### Backend (Python files)

```bash
scp backend/*.py ekin@95.216.72.161:~/mml-hackatune-26/discovery/
```

### Restart the service

```bash
ssh ekin@95.216.72.161 'sudo systemctl restart slu-discovery'
```

### Verify

```bash
ssh ekin@95.216.72.161 'curl -s http://localhost:8001/api/health'
# → {"status":"ok","tracks":50000,"users":462}
```

---

## Common operations

### Tail live logs

```bash
ssh ekin@95.216.72.161 'sudo journalctl -u slu-discovery -f'
```

### Service is stuck / won't stop

```bash
ssh ekin@95.216.72.161 'sudo systemctl kill -s SIGKILL slu-discovery && sudo systemctl start slu-discovery'
```

### Check audio is being served locally

```bash
curl -sI http://95.216.72.161:8001/audio/1000000.mp3 | head -2
# → HTTP/1.1 200 OK
```

### Smoke-test the API

```bash
ssh ekin@95.216.72.161 '
curl -s -X POST http://localhost:8001/api/chat \
  -H "Content-Type: application/json" \
  -d "{\"messages\":[{\"role\":\"user\",\"content\":\"chill piano\"}],\"limit\":2}" \
  | python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d[\"tracks\"]), \"tracks,\", d[\"filterSummary\"])"
'
```

---

## Re-run learned weights (optional, slow)

Requires Cyanite API key and ~2 hours (rate-limited fetches for any uncached tracks).

```bash
ssh ekin@95.216.72.161
cd ~/mml-hackatune-26/discovery
source /home/ekin/mml-hackatune-26/webapp/venv/bin/activate
python compute_learned_weights.py
# → writes data/learned_weights.json
sudo systemctl restart slu-discovery
```

---

## Rebuild the Rust query binary

Only needed if you modify the Rust harvester code.
Requires Rust toolchain on the server (already installed).

```bash
ssh ekin@95.216.72.161
cd ~/mml-hackatune-26/harvester
cargo build --release --bin query
# binary at: target/release/query
```

---

## Database schema (quick reference)

| Table | Contents |
|---|---|
| `tracks` | 50 k rows: cyanite_id, jamendo_id, name, artist, duration |
| `jamendo_track` | Jamendo metadata: name, artist, duration, license |
| `cyanite_tag` | Cyanite model outputs: (cyanite_id, model, tag, score) — ~10 k tracks |
| `cyanite_track` | Per-track valence, arousal, BPM, key, energy, vocal fields |
| `vec_mel` | 288-dim mel embeddings (pgvector) |
| `vec_chroma` | 48-dim chroma embeddings (pgvector) |
| `vec_mfcc` | 40-dim MFCC embeddings (pgvector) |
| `vec_tonnetz` | 6-dim Tonnetz embeddings (pgvector) |
| `rhythm`, `pitch`, `spectral`, `spectral_aubio`, `rms`, `zcr`, `notes` | Scalar audio features |

Key index for tag search:
```sql
CREATE INDEX idx_cyanite_tag_model_tag ON cyanite_tag(model, tag, cyanite_id, score);
```
