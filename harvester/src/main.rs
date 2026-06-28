//! `harvest` — parallel Jamendo MP3 ingester (Postgres-backed).
//!
//! Pipeline (per worker): a **download stage** claims pending tracks from the
//! shared queue (`FOR UPDATE SKIP LOCKED`) and fetches MP3s with bounded,
//! rate-limited concurrency; each finished file is handed over a bounded channel
//! to a **featurize stage** (CPU pool) that runs the DSP and writes Postgres.
//! Download I/O and CPU featurization overlap; the bounded channel back-pressures
//! downloads so disk can't run ahead. Run many workers / containers in parallel.

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use harvester::download::{Downloader, Fetched};
use harvester::{db, features, meta};
use sqlx::PgPool;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Parser)]
#[command(name = "harvest", about = "Parallel Jamendo MP3 feature ingester (Postgres)")]
struct Cli {
    #[arg(long, env = "DATABASE_URL", default_value = "postgres://harvest:harvest@localhost:5432/harvest")]
    database_url: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Load tracks.csv metadata (artist + album block) into Postgres.
    Init {
        #[arg(long, default_value = "../data/tracks.csv")]
        tracks: String,
    },
    /// Claim + download + featurize pending tracks. Run N of these in parallel.
    Run {
        #[arg(long, default_value = "corpus/audio")]
        out: String,
        /// CPU featurize workers (default: detected cores).
        #[arg(long, default_value_t = 0)]
        concurrency: usize,
        /// Parallel downloads (I/O bound; overlaps with featurization).
        #[arg(long, default_value_t = 8)]
        dl_concurrency: usize,
        /// Global download rate cap (requests/second) per worker.
        #[arg(long, default_value_t = 5)]
        rps: u32,
        #[arg(long, default_value_t = 60)]
        timeout: u64,
        /// Tracks claimed per round from the shared queue. Small = less over-claim.
        #[arg(long, default_value_t = 16)]
        batch: usize,
        #[arg(long, default_value_t = 30)]
        stale_minutes: i64,
        /// Delete each MP3 once featurized (signatures already live in Postgres).
        /// Essential on small disks — keeps usage bounded to in-flight files.
        #[arg(long, default_value_t = false)]
        delete_after: bool,
        /// Analyze only a central N-second window (big speedup; aggregate stats
        /// barely change). Omit to featurize the whole track.
        #[arg(long)]
        segment: Option<f32>,
        /// Stop after this worker has processed N tracks (smoke test).
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long, env = "WORKER_ID")]
        worker_id: Option<String>,
    },
    /// Print progress counts.
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Init { tracks } => {
            let pool = db::connect(&cli.database_url, 4).await?;
            db::migrate(&pool).await?;
            let metas = meta::load_tracks(&tracks)?;
            let n = db::upsert_meta(&pool, &metas).await?;
            println!("loaded {n} track metadata rows into Postgres");
        }
        Cmd::Status => {
            let pool = db::connect(&cli.database_url, 2).await?;
            db::migrate(&pool).await?;
            let (t, dl, f, p, x) = db::counts(&pool).await?;
            println!("tracks={t}  downloaded={dl}  featurized={f}  processing={p}  failed={x}");
        }
        Cmd::Run { out, concurrency, dl_concurrency, rps, timeout, batch, stale_minutes, delete_after, segment, limit, worker_id } => {
            let feat = if concurrency == 0 {
                std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
            } else {
                concurrency
            };
            run(&cli.database_url, out, feat, dl_concurrency, rps, timeout, batch, stale_minutes, delete_after, segment, limit, worker_id).await?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run(
    url: &str,
    out: String,
    feat_concurrency: usize,
    dl_concurrency: usize,
    rps: u32,
    timeout: u64,
    batch: usize,
    stale_minutes: i64,
    delete_after: bool,
    segment: Option<f32>,
    limit: Option<usize>,
    worker_id: Option<String>,
) -> Result<()> {
    let worker = worker_id.unwrap_or_else(default_worker_id);
    // pool: one connection per featurizer (concurrent writers) + headroom for
    // the downloader's claim/failure queries.
    let pool = db::connect(url, feat_concurrency as u32 + 4).await?;
    db::migrate(&pool).await?;
    let downloader = Arc::new(Downloader::new(&out, rps, timeout)?);
    println!("[{worker}] up | featurize={feat_concurrency} downloads={dl_concurrency} rps={rps} out={out}");

    // bounded channel = back-pressure: at most ~2x featurizers of downloaded-
    // but-unfeaturized files sit on disk at once.
    let (tx, rx) = tokio::sync::mpsc::channel::<(db::Pending, Fetched)>(feat_concurrency * 2);

    // ---- download stage ----
    let producer = {
        let pool = pool.clone();
        let dl = downloader.clone();
        let worker = worker.clone();
        tokio::spawn(async move {
            let mut processed = 0usize;
            'outer: loop {
                let claimed = db::claim_batch(&pool, &worker, batch as i64, stale_minutes).await?;
                if claimed.is_empty() {
                    break;
                }
                let mut downloads = futures::stream::iter(claimed.into_iter().map(|p| {
                    let dl = dl.clone();
                    async move {
                        let res = dl.fetch(&p.jamendo_id).await;
                        (p, res)
                    }
                }))
                .buffer_unordered(dl_concurrency);

                while let Some((p, res)) = downloads.next().await {
                    match res {
                        Ok(f) => {
                            // mark downloaded NOW (not after featurize) so the
                            // `downloaded` count reflects true download progress.
                            let _ = db::set_download(&pool, &p.cyanite_id, &f.path, f.bytes, &f.sha256).await;
                            if tx.send((p, f)).await.is_err() {
                                break 'outer; // featurizers gone
                            }
                        }
                        Err(e) => {
                            let _ = db::record_failure(&pool, &p.cyanite_id, "download", &e.to_string()).await;
                        }
                    }
                    processed += 1;
                    if let Some(l) = limit {
                        if processed >= l {
                            break 'outer;
                        }
                    }
                }
            }
            drop(tx); // close -> featurize stage drains then ends
            Ok::<_, anyhow::Error>(())
        })
    };

    // ---- featurize stage (CPU pool) ----
    let (ok, fail) = ReceiverStream::new(rx)
        .map(|(p, fetched)| {
            let pool = pool.clone();
            async move { featurize_and_store(&pool, p, fetched, delete_after, segment).await }
        })
        .buffer_unordered(feat_concurrency)
        .fold((0u64, 0u64), |(o, f), good| async move {
            if good {
                (o + 1, f)
            } else {
                (o, f + 1)
            }
        })
        .await;

    producer.await??;
    println!("[{worker}] done: {ok} featurized, {fail} failed.");
    Ok(())
}

/// Decode + featurize one downloaded file (catch_unwind isolated) and store it.
async fn featurize_and_store(pool: &PgPool, p: db::Pending, fetched: Fetched, delete_after: bool, segment: Option<f32>) -> bool {
    let path = fetched.path.clone();
    let computed = tokio::task::spawn_blocking(move || {
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let sig = features::decode(&path)?;
            features::extract(&sig, segment)
        }))
        .map_err(|_| anyhow!("panic during decode/feature extraction"))
        .and_then(|r| r)
    })
    .await;

    match computed {
        Ok(Ok(f)) => {
            match db::write_features(pool, &p.cyanite_id, &f).await {
                Ok(()) => {
                    if delete_after {
                        let _ = tokio::fs::remove_file(&fetched.path).await;
                    }
                    true
                }
                Err(e) => {
                    let _ = db::record_failure(pool, &p.cyanite_id, "db", &e.to_string()).await;
                    false
                }
            }
        }
        Ok(Err(e)) => {
            let _ = db::record_failure(pool, &p.cyanite_id, "features", &e.to_string()).await;
            false
        }
        Err(e) => {
            let _ = db::record_failure(pool, &p.cyanite_id, "join", &e.to_string()).await;
            false
        }
    }
}

fn default_worker_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "worker".into());
    format!("{host}-{}", std::process::id())
}
