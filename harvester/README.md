# harvester — parallel Jamendo MP3 feature ingester

Downloads the data-pack MP3s (rate-limited, resumable) and extracts a rich audio
feature set into **Postgres + pgvector**, one table per feature family. No
Cyanite API key needed — audio comes from Jamendo's public MP3 URLs.

Two binaries (one crate):
- **`harvest`** — `init` (load metadata), `run` (claim → download → featurize), `status`.
- **`query`** — read-only similarity / info, usable live while `harvest` runs.

## Parallelism

Workers share one Postgres queue via `SELECT … FOR UPDATE SKIP LOCKED`, so any
number of `harvest run` processes (or Docker replicas, or machines) drain it with
no double-work; crashed workers' in-flight rows are re-claimed after a timeout.
Inside a worker the pipeline is decoupled: a rate-limited **download stage** feeds
a bounded channel into a CPU **featurize pool** — I/O and DSP overlap, disk is
back-pressured.

## DSP stack (chosen by empirically compiling every candidate)

| Stage | Crate |
|---|---|
| decode → mono f64 @22050 | **rosa** (symphonia) — ffmpeg fallback |
| mel stack — 3 windows (low 1024 / mid 8192 / high 32768) | **rosa** |
| chroma stack (12-bin ×3) + 5-octave `chroma_cqt` + `tonnetz` | **rosa** |
| MFCC, spectral centroid/rolloff/bandwidth/flatness, rms, zcr, tempo, **beat tracking**, **pyin** pitch+voicing, tuning | **rosa** |
| spectral **flux** | **dasp_rs** |
| spectral **skewness/kurtosis/slope/decrease**, tempo **confidence**, onset rate, **notes** | **aubio** |

Every per-frame feature carries both **L2 (mean/std)** and **robust L1
(median/MAD)** stats. `aubio` is GPL-3.0 and its bundled C needs a CFLAGS flag on
GCC ≥ 14 — baked into `.cargo/config.toml` so `cargo build` just works.

## Similarity

- **pgvector cosine** over per-axis HNSW tables (`vec_mel`, `vec_chroma`,
  `vec_tonnetz`, `vec_mfcc`), combined with tunable weights at query time.
- **NCD** (Normalized Compression Distance, ~Kolmogorov): `zstd` over the chroma
  sequence, and **FLAC** (audio-native) over a PCM signature.

```bash
query similar <libtr_id> --k 10                       # weighted multi-axis cosine
query similar <libtr_id> --metric ncd                 # zstd compression distance
query similar <libtr_id> --metric ncd-audio           # FLAC audio-native distance
query similar <libtr_id> --w-mel 2 --w-tonnetz 1      # tune axis weights
query info <libtr_id>                                 # tempo / pitch / spectral
```

---

## Run it — option A: Docker (self-contained)

Brings up Postgres+pgvector, a one-shot metadata `init`, and scalable workers:

```bash
docker compose up -d --build --scale worker=4     # 4 workers
docker compose run --rm worker status             # progress
```

## Run it — option B: native, no Docker

Needs a local Postgres with pgvector (the only thing Docker was providing):

```bash
# 1. one-time: install postgres + pgvector, create the db (asks for sudo)
./scripts/setup-local-pg.sh

export DATABASE_URL=postgres://harvest:harvest@localhost:5432/harvest

# 2. ALWAYS build release — debug DSP is far slower.
cargo build --release

# 3. load metadata once
./target/release/harvest init --tracks ../data/tracks.csv

# 4. the run (uses all cores; resumable, safe to Ctrl-C and re-run)
nohup ./target/release/harvest run \
    --out corpus/audio --concurrency $(nproc) --dl-concurrency 8 --rps 6 \
    > harvest.run.log 2>&1 &

# 5. query / progress any time (separate terminal), while it runs
./target/release/harvest status
./target/release/query similar <libtr_id> --metric ncd-audio
```

Run several `harvest run` processes at once and they share the queue.

### Knobs
- `--delete-after` — remove each MP3 once featurized (signatures live in
  Postgres). Use on small disks; omit to keep originals (~63 GB for the catalog).
- `--limit N` — process only N tracks (smoke test).
- `--rps` / `--dl-concurrency` — politeness + download parallelism per worker.
- `--segment N` — analyze only a central N-second window (faster, lower fidelity).

### Throughput (measured, release, 8 cores, full track)
~6 s/track → **≈ 13–17 h for the 10,561 data-pack tracks**. The cost is in
`chroma_cqt` / `pyin` / `aubio`, not STFT (profile with
`cargo run --release --example profile -- <mp3>`).

Per the Challenge Agreement, Cyanite outputs are event-only — delete `corpus/`
and the database after the event.
