//! Postgres store + work queue — one table per feature family, plus separate
//! per-axis pgvector tables (mel / chroma / tonnetz / mfcc) so similarity can be
//! combined with tunable weights at query time.
//!
//! Parallel workers share the queue via `FOR UPDATE SKIP LOCKED`; crashed
//! workers' in-flight rows are re-claimed after a stale timeout. Every per-frame
//! feature carries both L2 (mean/std) and robust L1 (median/MAD) stats.

use crate::features::{BandStats, TrackFeatures};
use anyhow::Result;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::Row;

#[derive(Debug, Clone)]
pub struct TrackMeta {
    pub cyanite_id: String,
    pub jamendo_id: String,
    pub name: String,
    pub artist: String,
    pub album_block: String,
    pub duration: Option<f64>,
    pub license: String,
}

#[derive(Debug, Clone)]
pub struct Pending {
    pub cyanite_id: String,
    pub jamendo_id: String,
}

/// Weights for the per-axis similarity combine (cosine distance per axis).
#[derive(Debug, Clone, Copy)]
pub struct AxisWeights {
    pub mel: f64,
    pub chroma: f64,
    pub tonnetz: f64,
    pub mfcc: f64,
}
impl Default for AxisWeights {
    fn default() -> Self {
        Self { mel: 1.0, chroma: 1.0, tonnetz: 0.5, mfcc: 1.0 }
    }
}

fn f32_blob(xs: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(xs.len() * 4);
    for x in xs {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn pgvec(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 8);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("{x}"));
    }
    s.push(']');
    s
}

pub async fn connect(url: &str, max_conns: u32) -> Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(max_conns)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect(url)
        .await?)
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::raw_sql(&schema_sql()).execute(pool).await?;
    Ok(())
}

pub async fn upsert_meta(pool: &PgPool, rows: &[TrackMeta]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut n = 0u64;
    for m in rows {
        sqlx::query(
            "INSERT INTO tracks(cyanite_id, jamendo_id, name, artist, album_block, duration_csv, license, feature_status)
             VALUES($1,$2,$3,$4,$5,$6,$7,'pending')
             ON CONFLICT(cyanite_id) DO UPDATE SET
               jamendo_id=EXCLUDED.jamendo_id, name=EXCLUDED.name, artist=EXCLUDED.artist,
               album_block=EXCLUDED.album_block, duration_csv=EXCLUDED.duration_csv, license=EXCLUDED.license",
        )
        .bind(&m.cyanite_id).bind(&m.jamendo_id).bind(&m.name).bind(&m.artist)
        .bind(&m.album_block).bind(m.duration).bind(&m.license)
        .execute(&mut *tx).await?;
        n += 1;
    }
    tx.commit().await?;
    Ok(n)
}

pub async fn claim_batch(pool: &PgPool, worker: &str, n: i64, stale_minutes: i64) -> Result<Vec<Pending>> {
    let rows = sqlx::query(
        "WITH picked AS (
             SELECT cyanite_id FROM tracks
             WHERE feature_status='pending'
                OR (feature_status='processing' AND claimed_at < now() - make_interval(mins => $3::int))
             ORDER BY cyanite_id FOR UPDATE SKIP LOCKED LIMIT $2
         )
         UPDATE tracks t SET feature_status='processing', worker_id=$1, claimed_at=now()
           FROM picked WHERE t.cyanite_id = picked.cyanite_id
        RETURNING t.cyanite_id, t.jamendo_id",
    )
    .bind(worker).bind(n).bind(stale_minutes)
    .fetch_all(pool).await?;
    Ok(rows.into_iter()
        .map(|r| Pending { cyanite_id: r.get("cyanite_id"), jamendo_id: r.get("jamendo_id") })
        .collect())
}

pub async fn set_download(pool: &PgPool, cyanite_id: &str, path: &str, bytes: i64, sha: &str) -> Result<()> {
    sqlx::query("UPDATE tracks SET download_status='done', mp3_path=$2, mp3_bytes=$3, sha256=$4, updated_at=now() WHERE cyanite_id=$1")
        .bind(cyanite_id).bind(path).bind(bytes).bind(sha).execute(pool).await?;
    Ok(())
}

pub async fn record_failure(pool: &PgPool, cyanite_id: &str, stage: &str, reason: &str) -> Result<()> {
    // Truncate by char (codepoint), never by byte — a byte slice can split a
    // multi-byte UTF-8 sequence and panic outside the per-file catch_unwind.
    let reason: String = reason.chars().take(500).collect();
    let reason = reason.as_str();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO failures(cyanite_id, stage, reason, ts) VALUES($1,$2,$3,now())")
        .bind(cyanite_id).bind(stage).bind(reason).execute(&mut *tx).await?;
    sqlx::query("UPDATE tracks SET feature_status='failed', error=$2, updated_at=now() WHERE cyanite_id=$1")
        .bind(cyanite_id).bind(reason).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Write every feature family + per-axis vectors + compression sigs in one tx.
pub async fn write_features(pool: &PgPool, cyanite_id: &str, f: &TrackFeatures) -> Result<()> {
    let mut tx = pool.begin().await?;

    // grid stacks (mel + chroma): one row per (track, level)
    for g in &f.mel_stack {
        write_grid(&mut tx, "mel_stack", cyanite_id, g).await?;
    }
    for g in &f.chroma_stack {
        write_grid(&mut tx, "chroma_stack", cyanite_id, g).await?;
    }

    write_band(&mut tx, "chroma_cqt5", cyanite_id, &f.chroma_cqt5).await?;
    write_band(&mut tx, "tonnetz", cyanite_id, &f.tonnetz).await?;
    write_band(&mut tx, "mfcc", cyanite_id, &f.mfcc).await?;

    // spectral: L2+L1 per descriptor
    let s = &f.spectral;
    sqlx::query(
        "INSERT INTO spectral(cyanite_id,
            centroid_mean,centroid_std,centroid_median,centroid_mad,
            rolloff_mean,rolloff_std,rolloff_median,rolloff_mad,
            bandwidth_mean,bandwidth_std,bandwidth_median,bandwidth_mad,
            flatness_mean,flatness_std,flatness_median,flatness_mad,
            flux_mean,flux_std,flux_median,flux_mad)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)
         ON CONFLICT(cyanite_id) DO UPDATE SET
            centroid_mean=EXCLUDED.centroid_mean,centroid_std=EXCLUDED.centroid_std,centroid_median=EXCLUDED.centroid_median,centroid_mad=EXCLUDED.centroid_mad,
            rolloff_mean=EXCLUDED.rolloff_mean,rolloff_std=EXCLUDED.rolloff_std,rolloff_median=EXCLUDED.rolloff_median,rolloff_mad=EXCLUDED.rolloff_mad,
            bandwidth_mean=EXCLUDED.bandwidth_mean,bandwidth_std=EXCLUDED.bandwidth_std,bandwidth_median=EXCLUDED.bandwidth_median,bandwidth_mad=EXCLUDED.bandwidth_mad,
            flatness_mean=EXCLUDED.flatness_mean,flatness_std=EXCLUDED.flatness_std,flatness_median=EXCLUDED.flatness_median,flatness_mad=EXCLUDED.flatness_mad,
            flux_mean=EXCLUDED.flux_mean,flux_std=EXCLUDED.flux_std,flux_median=EXCLUDED.flux_median,flux_mad=EXCLUDED.flux_mad",
    )
    .bind(cyanite_id)
    .bind(s.centroid.mean).bind(s.centroid.std).bind(s.centroid.median).bind(s.centroid.mad)
    .bind(s.rolloff.mean).bind(s.rolloff.std).bind(s.rolloff.median).bind(s.rolloff.mad)
    .bind(s.bandwidth.mean).bind(s.bandwidth.std).bind(s.bandwidth.median).bind(s.bandwidth.mad)
    .bind(s.flatness.mean).bind(s.flatness.std).bind(s.flatness.median).bind(s.flatness.mad)
    .bind(s.flux.mean).bind(s.flux.std).bind(s.flux.median).bind(s.flux.mad)
    .execute(&mut *tx).await?;

    write_stat4(&mut tx, "rms", cyanite_id, &f.rms).await?;
    write_stat4(&mut tx, "zcr", cyanite_id, &f.zcr).await?;

    // rhythm (+ aubio tempo confidence / onset rate)
    let (tempo_conf, onset_rate) = f.aubio.as_ref().map(|a| (a.tempo_confidence, a.onset_rate)).unwrap_or((0.0, 0.0));
    sqlx::query(
        "INSERT INTO rhythm(cyanite_id, tempo_bpm, beat_bpm, beat_count, beat_regularity, tempo_confidence, onset_rate)
         VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT(cyanite_id) DO UPDATE SET tempo_bpm=EXCLUDED.tempo_bpm, beat_bpm=EXCLUDED.beat_bpm,
           beat_count=EXCLUDED.beat_count, beat_regularity=EXCLUDED.beat_regularity,
           tempo_confidence=EXCLUDED.tempo_confidence, onset_rate=EXCLUDED.onset_rate",
    )
    .bind(cyanite_id).bind(f.tempo_bpm).bind(f.beat_bpm).bind(f.beat_count)
    .bind(f.beat_regularity).bind(tempo_conf).bind(onset_rate)
    .execute(&mut *tx).await?;

    // pitch + tuning
    sqlx::query(
        "INSERT INTO pitch(cyanite_id, median_hz, voiced_frac, voiced_conf, tuning)
         VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(cyanite_id) DO UPDATE SET median_hz=EXCLUDED.median_hz,
           voiced_frac=EXCLUDED.voiced_frac, voiced_conf=EXCLUDED.voiced_conf, tuning=EXCLUDED.tuning",
    )
    .bind(cyanite_id).bind(f.pitch_median_hz).bind(f.voiced_frac).bind(f.voiced_conf).bind(f.tuning)
    .execute(&mut *tx).await?;

    // notes + aubio higher-order moments
    if let Some(a) = &f.aubio {
        sqlx::query(
            "INSERT INTO notes(cyanite_id, note_count, note_mean_dur) VALUES($1,$2,$3)
             ON CONFLICT(cyanite_id) DO UPDATE SET note_count=EXCLUDED.note_count, note_mean_dur=EXCLUDED.note_mean_dur")
            .bind(cyanite_id).bind(a.note_count).bind(a.note_mean_dur).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO spectral_aubio(cyanite_id, skewness_mean, skewness_std, kurtosis_mean, kurtosis_std,
                slope_mean, slope_std, decrease_mean, decrease_std)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT(cyanite_id) DO UPDATE SET skewness_mean=EXCLUDED.skewness_mean, skewness_std=EXCLUDED.skewness_std,
               kurtosis_mean=EXCLUDED.kurtosis_mean, kurtosis_std=EXCLUDED.kurtosis_std,
               slope_mean=EXCLUDED.slope_mean, slope_std=EXCLUDED.slope_std,
               decrease_mean=EXCLUDED.decrease_mean, decrease_std=EXCLUDED.decrease_std")
            .bind(cyanite_id)
            .bind(a.skewness_mean).bind(a.skewness_std).bind(a.kurtosis_mean).bind(a.kurtosis_std)
            .bind(a.slope_mean).bind(a.slope_std).bind(a.decrease_mean).bind(a.decrease_std)
            .execute(&mut *tx).await?;
    }

    // per-axis similarity vectors (kept separate)
    write_vec(&mut tx, "vec_mel", cyanite_id, &crate::features::vec_mel(f)).await?;
    write_vec(&mut tx, "vec_chroma", cyanite_id, &crate::features::vec_chroma(f)).await?;
    write_vec(&mut tx, "vec_tonnetz", cyanite_id, &crate::features::vec_tonnetz(f)).await?;
    write_vec(&mut tx, "vec_mfcc", cyanite_id, &crate::features::vec_mfcc(f)).await?;

    // compression signatures: zstd(chroma seq) + FLAC(pcm)
    let csize = crate::compression::clen(&f.comp_sig) as i32;
    let fsize = crate::compression::flac_len(&f.comp_pcm) as i32;
    sqlx::query(
        "INSERT INTO compression(cyanite_id, sig, csize, pcm, fsize) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(cyanite_id) DO UPDATE SET sig=EXCLUDED.sig, csize=EXCLUDED.csize, pcm=EXCLUDED.pcm, fsize=EXCLUDED.fsize")
        .bind(cyanite_id).bind(&f.comp_sig).bind(csize).bind(&f.comp_pcm).bind(fsize)
        .execute(&mut *tx).await?;

    sqlx::query("UPDATE tracks SET feature_status='done', duration_audio=$2, error=NULL, updated_at=now() WHERE cyanite_id=$1")
        .bind(cyanite_id).bind(f.duration_s).execute(&mut *tx).await?;

    tx.commit().await?;
    Ok(())
}

type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

async fn write_grid(tx: &mut Tx<'_>, table: &str, id: &str, g: &crate::features::GridSummary) -> Result<()> {
    let sql = format!(
        "INSERT INTO {table}(cyanite_id, level, win, hop, n_bands, n_frames, mean, std, median, mad)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         ON CONFLICT(cyanite_id, level) DO UPDATE SET win=EXCLUDED.win, hop=EXCLUDED.hop,
           n_bands=EXCLUDED.n_bands, n_frames=EXCLUDED.n_frames, mean=EXCLUDED.mean,
           std=EXCLUDED.std, median=EXCLUDED.median, mad=EXCLUDED.mad"
    );
    sqlx::query(&sql)
        .bind(id).bind(g.level).bind(g.win as i64).bind(g.hop as i64)
        .bind(g.n_bands as i64).bind(g.n_frames as i64)
        .bind(f32_blob(&g.stats.mean)).bind(f32_blob(&g.stats.std))
        .bind(f32_blob(&g.stats.median)).bind(f32_blob(&g.stats.mad))
        .execute(&mut **tx).await?;
    Ok(())
}

async fn write_band(tx: &mut Tx<'_>, table: &str, id: &str, b: &BandStats) -> Result<()> {
    let sql = format!(
        "INSERT INTO {table}(cyanite_id, mean, std, median, mad) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(cyanite_id) DO UPDATE SET mean=EXCLUDED.mean, std=EXCLUDED.std, median=EXCLUDED.median, mad=EXCLUDED.mad"
    );
    sqlx::query(&sql)
        .bind(id).bind(f32_blob(&b.mean)).bind(f32_blob(&b.std)).bind(f32_blob(&b.median)).bind(f32_blob(&b.mad))
        .execute(&mut **tx).await?;
    Ok(())
}

async fn write_stat4(tx: &mut Tx<'_>, table: &str, id: &str, s: &crate::features::Stats) -> Result<()> {
    let sql = format!(
        "INSERT INTO {table}(cyanite_id, mean, std, median, mad) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(cyanite_id) DO UPDATE SET mean=EXCLUDED.mean, std=EXCLUDED.std, median=EXCLUDED.median, mad=EXCLUDED.mad"
    );
    sqlx::query(&sql).bind(id).bind(s.mean).bind(s.std).bind(s.median).bind(s.mad).execute(&mut **tx).await?;
    Ok(())
}

async fn write_vec(tx: &mut Tx<'_>, table: &str, id: &str, v: &[f32]) -> Result<()> {
    let sql = format!(
        "INSERT INTO {table}(cyanite_id, embedding) VALUES($1, $2::vector)
         ON CONFLICT(cyanite_id) DO UPDATE SET embedding=EXCLUDED.embedding"
    );
    sqlx::query(&sql).bind(id).bind(pgvec(v)).execute(&mut **tx).await?;
    Ok(())
}

/// Weighted multi-axis cosine KNN (mel/chroma/tonnetz/mfcc). Scans the joined
/// vector tables (fine at 10k rows); single-axis HNSW indexes exist for cheaper
/// per-axis queries.
pub async fn vec_knn(pool: &PgPool, id: &str, k: i64, w: AxisWeights) -> Result<Vec<(String, f64)>> {
    let rows = sqlx::query(
        "WITH q AS (
            SELECT (SELECT embedding FROM vec_mel WHERE cyanite_id=$1)     AS qmel,
                   (SELECT embedding FROM vec_chroma WHERE cyanite_id=$1)  AS qchr,
                   (SELECT embedding FROM vec_tonnetz WHERE cyanite_id=$1) AS qton,
                   (SELECT embedding FROM vec_mfcc WHERE cyanite_id=$1)    AS qmf)
         SELECT m.cyanite_id,
                $3*(m.embedding <=> q.qmel) + $4*(c.embedding <=> q.qchr)
              + $5*(t.embedding <=> q.qton) + $6*(f.embedding <=> q.qmf) AS dist
           FROM vec_mel m
           JOIN vec_chroma c USING(cyanite_id)
           JOIN vec_tonnetz t USING(cyanite_id)
           JOIN vec_mfcc f USING(cyanite_id), q
          WHERE m.cyanite_id <> $1
          ORDER BY dist LIMIT $2",
    )
    .bind(id).bind(k).bind(w.mel).bind(w.chroma).bind(w.tonnetz).bind(w.mfcc)
    .fetch_all(pool).await?;
    // dist is NULL when the seed lacks one of its four vec_* rows; skip rather than
    // panic on a non-nullable get.
    Ok(rows.into_iter()
        .filter_map(|r| r.try_get::<f64, _>("dist").ok()
            .map(|d| (r.get::<String, _>("cyanite_id"), d)))
        .collect())
}

/// Weighted multi-axis cosine KNN from QUERY VECTORS (e.g. the features of an
/// uploaded mp3/wav) rather than an existing track id — the audio-upload "find
/// similar" path. Vectors are passed as pgvector text literals cast to `vector`.
pub async fn vec_knn_from(
    pool: &PgPool, qmel: &[f32], qchr: &[f32], qton: &[f32], qmf: &[f32], k: i64, w: AxisWeights,
) -> Result<Vec<(String, f64)>> {
    fn vstr(v: &[f32]) -> String {
        let mut s = String::with_capacity(v.len() * 8 + 2);
        s.push('[');
        for (i, x) in v.iter().enumerate() {
            if i > 0 { s.push(','); }
            s.push_str(&x.to_string());
        }
        s.push(']');
        s
    }
    let rows = sqlx::query(
        "SELECT m.cyanite_id,
                $1*(m.embedding <=> $5::vector) + $2*(c.embedding <=> $6::vector)
              + $3*(t.embedding <=> $7::vector) + $4*(f.embedding <=> $8::vector) AS dist
           FROM vec_mel m
           JOIN vec_chroma c USING(cyanite_id)
           JOIN vec_tonnetz t USING(cyanite_id)
           JOIN vec_mfcc f USING(cyanite_id)
          ORDER BY dist LIMIT $9",
    )
    .bind(w.mel).bind(w.chroma).bind(w.tonnetz).bind(w.mfcc)
    .bind(vstr(qmel)).bind(vstr(qchr)).bind(vstr(qton)).bind(vstr(qmf))
    .bind(k)
    .fetch_all(pool).await?;
    Ok(rows.into_iter()
        .filter_map(|r| r.try_get::<f64, _>("dist").ok()
            .map(|d| (r.get::<String, _>("cyanite_id"), d)))
        .collect())
}

/// Compression signatures for NCD: (chroma sig, csize, pcm, fsize).
pub async fn fetch_sig(pool: &PgPool, id: &str) -> Result<Option<(Vec<u8>, i32, Vec<u8>, i32)>> {
    let row = sqlx::query("SELECT sig, csize, pcm, fsize FROM compression WHERE cyanite_id=$1")
        .bind(id).fetch_optional(pool).await?;
    Ok(row.map(|r| (r.get("sig"), r.get("csize"), r.get("pcm"), r.get("fsize"))))
}

pub async fn fetch_sigs(pool: &PgPool, ids: &[String]) -> Result<Vec<(String, Vec<u8>, i32, Vec<u8>, i32)>> {
    let rows = sqlx::query("SELECT cyanite_id, sig, csize, pcm, fsize FROM compression WHERE cyanite_id = ANY($1)")
        .bind(ids).fetch_all(pool).await?;
    Ok(rows.into_iter()
        .map(|r| (r.get("cyanite_id"), r.get("sig"), r.get("csize"), r.get("pcm"), r.get("fsize")))
        .collect())
}

pub async fn counts(pool: &PgPool) -> Result<(i64, i64, i64, i64, i64)> {
    let r = sqlx::query(
        "SELECT COUNT(*) AS total,
                COUNT(*) FILTER (WHERE download_status='done') AS dl,
                COUNT(*) FILTER (WHERE feature_status='done') AS feat,
                COUNT(*) FILTER (WHERE feature_status='processing') AS proc,
                COUNT(*) FILTER (WHERE feature_status='failed') AS fail
         FROM tracks",
    )
    .fetch_one(pool).await?;
    Ok((r.get("total"), r.get("dl"), r.get("feat"), r.get("proc"), r.get("fail")))
}

fn schema_sql() -> String {
    use crate::features::{DIM_CHROMA, DIM_MEL, DIM_MFCC, DIM_TONNETZ};
    format!(
        r#"
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS tracks(
    cyanite_id TEXT PRIMARY KEY,
    jamendo_id TEXT NOT NULL,
    name TEXT, artist TEXT, album_block TEXT,
    duration_csv DOUBLE PRECISION, license TEXT,
    mp3_path TEXT, mp3_bytes BIGINT, sha256 TEXT,
    download_status TEXT,
    feature_status TEXT NOT NULL DEFAULT 'pending',
    worker_id TEXT, claimed_at TIMESTAMPTZ,
    duration_audio REAL, error TEXT, updated_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_tracks_fstatus ON tracks(feature_status);
CREATE INDEX IF NOT EXISTS idx_tracks_artist  ON tracks(artist);

-- grid stacks (one row per resolution level)
CREATE TABLE IF NOT EXISTS mel_stack(
    cyanite_id TEXT, level INTEGER, win BIGINT, hop BIGINT, n_bands BIGINT, n_frames BIGINT,
    mean BYTEA, std BYTEA, median BYTEA, mad BYTEA, PRIMARY KEY(cyanite_id, level));
CREATE TABLE IF NOT EXISTS chroma_stack(
    cyanite_id TEXT, level INTEGER, win BIGINT, hop BIGINT, n_bands BIGINT, n_frames BIGINT,
    mean BYTEA, std BYTEA, median BYTEA, mad BYTEA, PRIMARY KEY(cyanite_id, level));

-- single-resolution band features (mean/std/median/mad blobs)
CREATE TABLE IF NOT EXISTS chroma_cqt5(cyanite_id TEXT PRIMARY KEY, mean BYTEA, std BYTEA, median BYTEA, mad BYTEA);
CREATE TABLE IF NOT EXISTS tonnetz(cyanite_id TEXT PRIMARY KEY, mean BYTEA, std BYTEA, median BYTEA, mad BYTEA);
CREATE TABLE IF NOT EXISTS mfcc(cyanite_id TEXT PRIMARY KEY, mean BYTEA, std BYTEA, median BYTEA, mad BYTEA);

CREATE TABLE IF NOT EXISTS spectral(
    cyanite_id TEXT PRIMARY KEY,
    centroid_mean REAL, centroid_std REAL, centroid_median REAL, centroid_mad REAL,
    rolloff_mean REAL, rolloff_std REAL, rolloff_median REAL, rolloff_mad REAL,
    bandwidth_mean REAL, bandwidth_std REAL, bandwidth_median REAL, bandwidth_mad REAL,
    flatness_mean REAL, flatness_std REAL, flatness_median REAL, flatness_mad REAL,
    flux_mean REAL, flux_std REAL, flux_median REAL, flux_mad REAL);
CREATE TABLE IF NOT EXISTS spectral_aubio(
    cyanite_id TEXT PRIMARY KEY,
    skewness_mean REAL, skewness_std REAL, kurtosis_mean REAL, kurtosis_std REAL,
    slope_mean REAL, slope_std REAL, decrease_mean REAL, decrease_std REAL);
CREATE TABLE IF NOT EXISTS rms(cyanite_id TEXT PRIMARY KEY, mean REAL, std REAL, median REAL, mad REAL);
CREATE TABLE IF NOT EXISTS zcr(cyanite_id TEXT PRIMARY KEY, mean REAL, std REAL, median REAL, mad REAL);

CREATE TABLE IF NOT EXISTS rhythm(
    cyanite_id TEXT PRIMARY KEY, tempo_bpm REAL, beat_bpm REAL, beat_count INTEGER,
    beat_regularity REAL, tempo_confidence REAL, onset_rate REAL);
CREATE TABLE IF NOT EXISTS pitch(
    cyanite_id TEXT PRIMARY KEY, median_hz REAL, voiced_frac REAL, voiced_conf REAL, tuning REAL);
CREATE TABLE IF NOT EXISTS notes(cyanite_id TEXT PRIMARY KEY, note_count REAL, note_mean_dur REAL);

CREATE TABLE IF NOT EXISTS failures(cyanite_id TEXT, stage TEXT, reason TEXT, ts TIMESTAMPTZ);

-- per-axis similarity vectors (combined with weights at query time)
CREATE TABLE IF NOT EXISTS vec_mel(cyanite_id TEXT PRIMARY KEY, embedding vector({DIM_MEL}));
CREATE TABLE IF NOT EXISTS vec_chroma(cyanite_id TEXT PRIMARY KEY, embedding vector({DIM_CHROMA}));
CREATE TABLE IF NOT EXISTS vec_tonnetz(cyanite_id TEXT PRIMARY KEY, embedding vector({DIM_TONNETZ}));
CREATE TABLE IF NOT EXISTS vec_mfcc(cyanite_id TEXT PRIMARY KEY, embedding vector({DIM_MFCC}));
CREATE INDEX IF NOT EXISTS vec_mel_hnsw     ON vec_mel     USING hnsw (embedding vector_cosine_ops);
CREATE INDEX IF NOT EXISTS vec_chroma_hnsw  ON vec_chroma  USING hnsw (embedding vector_cosine_ops);
CREATE INDEX IF NOT EXISTS vec_tonnetz_hnsw ON vec_tonnetz USING hnsw (embedding vector_cosine_ops);
CREATE INDEX IF NOT EXISTS vec_mfcc_hnsw    ON vec_mfcc    USING hnsw (embedding vector_cosine_ops);

-- compression signatures for NCD: zstd(chroma) + FLAC(pcm)
CREATE TABLE IF NOT EXISTS compression(
    cyanite_id TEXT PRIMARY KEY, sig BYTEA, csize INTEGER, pcm BYTEA, fsize INTEGER);
"#
    )
}
