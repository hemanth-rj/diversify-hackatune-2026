//! `spectrogram` — background job: compute the FULL log-mel spectrogram for every
//! analyzed track and store it (quantized + zstd-compressed) in Postgres.
//!
//! The harvester only stored time-AVERAGED mel means; this keeps the whole
//! time×frequency matrix. Representation (per the design): 128-mel, n_fft 2048,
//! hop 512 @ 22.05 kHz, power→dB in [-80,0] mapped to u8 (0.31 dB/step), then
//! zstd. Idempotent and resumable: skips tracks already present.

use anyhow::Result;
use futures::stream::{self, StreamExt};
use harvester::features::{decode, SR};
use rosa::stft::MelFilterParams;
use rosa::{mel_spectrogram, MelSpectrogramParams};
use std::collections::HashSet;
use std::path::Path;

const N_FFT: usize = 2048;
const HOP: usize = 512;
const DB_MIN: f64 = -80.0;
const DB_MAX: f64 = 0.0;

/// Decode + log-mel + quantize + zstd. Returns (n_mels, n_frames, compressed).
fn compute(path: &str) -> Result<(i32, i32, Vec<u8>)> {
    let sig = decode(path)?;
    anyhow::ensure!(sig.len() >= N_FFT, "signal too short ({} samples)", sig.len());
    let mut mp = MelSpectrogramParams::new(SR as f64);
    mp.stft.n_fft = N_FFT;
    mp.stft.win_length = N_FFT;
    mp.stft.hop_length = HOP;
    mp.mel = MelFilterParams::new(SR as f64, N_FFT);
    let mel = mel_spectrogram(&sig, &mp);
    let (rows, cols) = (mel.rows(), mel.cols());
    let mut data = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        for t in 0..cols {
            let db = 10.0 * (mel.get(r, t) + 1e-10).log10();
            let c = ((db - DB_MIN) / (DB_MAX - DB_MIN)).clamp(0.0, 1.0);
            data.push((c * 255.0).round() as u8);
        }
    }
    let comp = zstd::encode_all(&data[..], 9)?;
    Ok((rows as i32, cols as i32, comp))
}

#[tokio::main]
async fn main() -> Result<()> {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://harvest:harvest@localhost:5432/harvest".into());
    let conc: usize = std::env::var("SPECTRO_CONCURRENCY").ok().and_then(|s| s.parse().ok()).unwrap_or(6);
    let pool = harvester::db::connect(&url, (conc as u32) + 2).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS spectrogram(
            cyanite_id text PRIMARY KEY, n_mels int, n_frames int, sr int, n_fft int, hop int,
            db_min real, db_max real, data bytea, created_at timestamptz NOT NULL DEFAULT now())",
    ).execute(&pool).await?;

    // mp3_path in the DB is a stale worker path (/root/...); resolve from the real
    // corpus dir + jamendo_id instead. Only ~12k of the 50k tracks have audio here.
    let corpus = std::env::var("CORPUS").unwrap_or_else(|_| {
        format!("{}/mml-hackatune-26/harvester/corpus/audio",
                std::env::var("HOME").unwrap_or_default())
    });
    let mut done: i64 = sqlx::query_scalar("SELECT count(*) FROM spectrogram").fetch_one(&pool).await?;
    println!("spectrogram job: {done} already stored, corpus {corpus}, concurrency {conc}");

    // keyset pagination over tracks so missing-file rows don't get re-scanned forever.
    let mut after = String::new();
    loop {
        let page = sqlx::query_as::<_, (String, String)>(
            "SELECT cyanite_id, jamendo_id FROM tracks
             WHERE feature_status='done' AND jamendo_id IS NOT NULL AND cyanite_id > $1
             ORDER BY cyanite_id LIMIT 256",
        ).bind(&after).fetch_all(&pool).await?;
        if page.is_empty() {
            break;
        }
        after = page.last().unwrap().0.clone();
        let ids: Vec<String> = page.iter().map(|(c, _)| c.clone()).collect();
        let existing: HashSet<String> = sqlx::query_scalar::<_, String>(
            "SELECT cyanite_id FROM spectrogram WHERE cyanite_id = ANY($1)",
        ).bind(&ids).fetch_all(&pool).await?.into_iter().collect();
        let todo: Vec<(String, String)> = page.into_iter()
            .filter(|(c, j)| !existing.contains(c) && Path::new(&format!("{corpus}/{j}.mp3")).exists())
            .collect();
        if todo.is_empty() {
            continue;
        }
        let results = stream::iter(todo)
            .map(|(cid, jam)| {
                let p = format!("{corpus}/{jam}.mp3");
                async move { (cid, tokio::task::spawn_blocking(move || compute(&p)).await) }
            })
            .buffer_unordered(conc)
            .collect::<Vec<_>>()
            .await;

        for (cid, res) in results {
            match res {
                Ok(Ok((n_mels, n_frames, data))) => {
                    let r = sqlx::query(
                        "INSERT INTO spectrogram(cyanite_id,n_mels,n_frames,sr,n_fft,hop,db_min,db_max,data)
                         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (cyanite_id) DO NOTHING",
                    )
                    .bind(&cid).bind(n_mels).bind(n_frames).bind(SR as i32)
                    .bind(N_FFT as i32).bind(HOP as i32).bind(DB_MIN as f32).bind(DB_MAX as f32).bind(data)
                    .execute(&pool).await;
                    if r.is_ok() {
                        done += 1;
                    }
                }
                Ok(Err(e)) => eprintln!("skip {cid}: {e}"),
                Err(e) => eprintln!("panic {cid}: {e}"),
            }
        }
        println!("spectrogram: {done} stored");
    }
    println!("spectrogram job complete: {done} stored");
    Ok(())
}
