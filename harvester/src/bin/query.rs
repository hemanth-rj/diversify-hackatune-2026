//! `query` — a separate read-only CLI over the same Postgres store, so you can
//! explore similarity and progress *while* `harvest` workers are still ingesting
//! (Postgres handles concurrent readers/writers fine).

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use harvester::{compression, db, features};
use sqlx::Row;

#[derive(Parser)]
#[command(name = "query", about = "Query the harvester feature store (live, read-only)")]
struct Cli {
    #[arg(long, env = "DATABASE_URL", default_value = "postgres://harvest:harvest@localhost:5432/harvest")]
    database_url: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ingestion progress counts.
    Status,
    /// Similar tracks. metric: vec | ncd | ncd-audio.
    Similar {
        id: String,
        #[arg(long, default_value_t = 10)]
        k: usize,
        #[arg(long, default_value = "vec")]
        metric: String,
        #[arg(long, default_value_t = 1.0)]
        w_mel: f64,
        #[arg(long, default_value_t = 1.0)]
        w_chroma: f64,
        #[arg(long, default_value_t = 0.5)]
        w_tonnetz: f64,
        #[arg(long, default_value_t = 1.0)]
        w_mfcc: f64,
    },
    /// Show a track's headline features (rhythm / pitch / spectral).
    Info { id: String },
    /// Similar catalog tracks to an arbitrary AUDIO FILE (mp3/wav/flac/…): decode →
    /// extract the same 382-d embedding as ingestion → pgvector kNN. Prints JSON.
    SimilarFile {
        path: String,
        #[arg(long, default_value_t = 12)]
        k: usize,
        #[arg(long, default_value_t = 1.0)]
        w_mel: f64,
        #[arg(long, default_value_t = 1.0)]
        w_chroma: f64,
        #[arg(long, default_value_t = 0.5)]
        w_tonnetz: f64,
        #[arg(long, default_value_t = 1.0)]
        w_mfcc: f64,
    },
    /// All-pairs NCD over a set of cyanite ids, for the NCD↔cosine Mantel experiment.
    /// metric: chroma (zstd over the chroma signature) | audio (FLAC over PCM).
    /// NCD is SYMMETRIZED — (ncd(i,j)+ncd(j,i))/2 — and each line also carries the
    /// magnitude/duration confound axes (csize, siglen) so the Python side can run a
    /// partial Mantel test. Prints one JSON object per unordered pair.
    NcdPairs {
        /// comma-separated cyanite ids
        ids: String,
        #[arg(long, default_value = "chroma")]
        metric: String,
    },
    /// Print the param-identical zstd chroma signature bytes of an arbitrary audio file
    /// (mp3/wav/…) as a JSON byte array, for query-by-humming contour matching.
    HumBytes { path: String },
    /// Print the DECODED 12-bin chroma frames of an arbitrary audio file as JSON:
    /// {nframes, nbins:12, chroma:[[f0..f11], ...]} — one row per frame, values
    /// dequantized to 0..1 floats. Reuses the exact same chroma the NCD comp_sig
    /// uses (mid-resolution chroma_stft, frame-major, already downsampled to
    /// ≤3000 frames), so the query contour is built from identical features to
    /// the catalog. Python does no zstd/reshape/format guessing.
    HumChroma { path: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let pool = db::connect(&cli.database_url, 4).await?;

    match cli.cmd {
        Cmd::Status => {
            let (t, dl, f, p, x) = db::counts(&pool).await?;
            println!("tracks={t}  downloaded={dl}  featurized={f}  processing={p}  failed={x}");
        }
        Cmd::Similar { id, k, metric, w_mel, w_chroma, w_tonnetz, w_mfcc } => {
            let w = db::AxisWeights { mel: w_mel, chroma: w_chroma, tonnetz: w_tonnetz, mfcc: w_mfcc };
            let wide = if metric == "vec" { k } else { (k * 5).max(k) };
            let cand = db::vec_knn(&pool, &id, wide as i64, w).await?;
            if metric == "vec" {
                for (cid, dist) in cand.into_iter().take(k) {
                    println!("{dist:.4}  {cid}");
                }
            } else {
                let seed = db::fetch_sig(&pool, &id).await?.ok_or_else(|| anyhow!("no signature for {id}"))?;
                let ids: Vec<String> = cand.iter().map(|(c, _)| c.clone()).collect();
                let sigs = db::fetch_sigs(&pool, &ids).await?;
                let audio = metric == "ncd-audio";
                let mut scored: Vec<(String, f32)> = sigs
                    .iter()
                    .map(|(cid, sig, cs, pcm, fs)| {
                        let n = if audio {
                            compression::ncd_flac(&seed.2, pcm, seed.3 as usize, *fs as usize)
                        } else {
                            compression::ncd(&seed.0, sig, seed.1 as usize, *cs as usize)
                        };
                        (cid.clone(), n)
                    })
                    .collect();
                scored.sort_by(|a, b| a.1.total_cmp(&b.1));
                for (cid, n) in scored.into_iter().take(k) {
                    println!("ncd={n:.4}  {cid}");
                }
            }
        }
        Cmd::SimilarFile { path, k, w_mel, w_chroma, w_tonnetz, w_mfcc } => {
            let sig = features::decode(&path)?;
            let f = features::extract(&sig, None)?;   // whole-track aggregates (matches ingestion)
            let w = db::AxisWeights { mel: w_mel, chroma: w_chroma, tonnetz: w_tonnetz, mfcc: w_mfcc };
            let cand = db::vec_knn_from(
                &pool, &features::vec_mel(&f), &features::vec_chroma(&f),
                &features::vec_tonnetz(&f), &features::vec_mfcc(&f), k as i64, w,
            ).await?;
            let out: Vec<_> = cand.iter()
                .map(|(c, d)| serde_json::json!({"cyanite_id": c, "dist": d}))
                .collect();
            println!("{}", serde_json::to_string(&out)?);
        }
        Cmd::NcdPairs { ids, metric } => {
            let idv: Vec<String> =
                ids.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
            let sigs = db::fetch_sigs(&pool, &idv).await?;   // (id, sig, csize, pcm, fsize)
            let audio = metric == "audio";
            for i in 0..sigs.len() {
                for j in (i + 1)..sigs.len() {
                    let a = &sigs[i];
                    let b = &sigs[j];
                    let n = if audio {
                        let n1 = compression::ncd_flac(&a.3, &b.3, a.4 as usize, b.4 as usize);
                        let n2 = compression::ncd_flac(&b.3, &a.3, b.4 as usize, a.4 as usize);
                        (n1 + n2) / 2.0
                    } else {
                        let n1 = compression::ncd(&a.1, &b.1, a.2 as usize, b.2 as usize);
                        let n2 = compression::ncd(&b.1, &a.1, b.2 as usize, a.2 as usize);
                        (n1 + n2) / 2.0
                    };
                    println!("{}", serde_json::json!({
                        "a": a.0, "b": b.0, "ncd": n,
                        "csize_a": a.2, "csize_b": b.2,
                        "siglen_a": a.1.len(), "siglen_b": b.1.len(),
                        "fsize_a": a.4, "fsize_b": b.4,
                    }));
                }
            }
        }
        Cmd::HumBytes { path } => {
            let f = features::extract(&features::decode(&path)?, None)?;
            println!("{}", serde_json::to_string(&f.comp_sig)?);   // JSON byte array
        }
        Cmd::HumChroma { path } => {
            // comp_sig is the un-zstd'd chroma byte sequence: frame-major, 12 bins
            // per frame, each byte = chroma.clamp(0,1)*255. Reshape + dequantize.
            let f = features::extract(&features::decode(&path)?, None)?;
            let nbins = 12usize;
            let nframes = f.comp_sig.len() / nbins;
            let mut chroma: Vec<Vec<f32>> = Vec::with_capacity(nframes);
            for t in 0..nframes {
                let mut row = Vec::with_capacity(nbins);
                for p in 0..nbins {
                    row.push(f.comp_sig[t * nbins + p] as f32 / 255.0);
                }
                chroma.push(row);
            }
            println!("{}", serde_json::to_string(&serde_json::json!({
                "nframes": nframes, "nbins": nbins, "chroma": chroma,
            }))?);
        }
        Cmd::Info { id } => {
            let r = sqlx::query(
                "SELECT t.name, t.artist, r.tempo_bpm, r.beat_bpm, r.beat_count, r.tempo_confidence,
                        p.median_hz, p.voiced_frac, p.voiced_conf, p.tuning,
                        s.centroid_median, s.flatness_median, n.note_count
                 FROM tracks t
                 LEFT JOIN rhythm r ON r.cyanite_id=t.cyanite_id
                 LEFT JOIN pitch p ON p.cyanite_id=t.cyanite_id
                 LEFT JOIN spectral s ON s.cyanite_id=t.cyanite_id
                 LEFT JOIN notes n ON n.cyanite_id=t.cyanite_id
                 WHERE t.cyanite_id=$1",
            )
            .bind(&id)
            .fetch_optional(&pool)
            .await?;
            match r {
                None => println!("no such track: {id}"),
                Some(r) => {
                    let name: Option<String> = r.get("name");
                    let artist: Option<String> = r.get("artist");
                    println!("{}  —  {}", name.unwrap_or_default(), artist.unwrap_or_default());
                    println!("  tempo {:?} bpm (beat {:?}, {:?} beats, conf {:?})",
                        r.get::<Option<f32>, _>("tempo_bpm"), r.get::<Option<f32>, _>("beat_bpm"),
                        r.get::<Option<i32>, _>("beat_count"), r.get::<Option<f32>, _>("tempo_confidence"));
                    println!("  pitch median {:?} Hz, voiced {:?} (conf {:?}), tuning {:?}",
                        r.get::<Option<f32>, _>("median_hz"), r.get::<Option<f32>, _>("voiced_frac"),
                        r.get::<Option<f32>, _>("voiced_conf"), r.get::<Option<f32>, _>("tuning"));
                    println!("  spectral centroid(med) {:?}, flatness(med) {:?}, notes {:?}",
                        r.get::<Option<f32>, _>("centroid_median"), r.get::<Option<f32>, _>("flatness_median"),
                        r.get::<Option<f32>, _>("note_count"));
                }
            }
        }
    }
    Ok(())
}
