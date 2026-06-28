//! Per-track audio feature extraction (rosa + aubio + dasp_rs).
//!
//! Every per-frame series is summarised with BOTH L2 (mean/std) and **robust L1
//! (median / MAD)** statistics, so outlier frames (a cough, a dropout) don't
//! skew the signature. Families:
//!   - multi-resolution **mel stack** (L<M<N<K) — timbre over time/freq scales
//!   - **chroma stack** (12-bin, same 4 resolutions) + **5-octave chroma_cqt** — harmony
//!   - **tonnetz** (6-dim tonal centroid) — polytonal / harmonic relations
//!   - MFCC, spectral centroid/rolloff/bandwidth/flatness/flux, rms, zcr — timbre
//!   - rhythm: tempo + **beat tracking** (bpm, beat count, regularity)
//!   - **pitch** (pyin: median f0, voiced fraction, voicing confidence) + tuning
//!   - aubio higher-order moments + tempo confidence + onset rate (see aubio_extra)
//!   - two compression signatures for NCD: a chroma byte sequence (zstd) and a
//!     downsampled PCM blob (FLAC, audio-native).
//!
//! rosa is f64; we cast to f32 only at the DB boundary. Robustness: decode falls
//! back to ffmpeg; NaN/Inf scrubbed; signals shorter than the largest window are
//! rejected; the orchestrator wraps everything in catch_unwind.

use anyhow::{anyhow, Result};
use rosa::stft::MelFilterParams;
use rosa::{
    beat_track, chroma_cqt, chroma_stft, estimate_tuning, fft_frequencies, mel_spectrogram, mfcc,
    onset_strength, pyin, rms, spectral_bandwidth, spectral_centroid, spectral_flatness,
    spectral_rolloff, spectrogram, tonnetz, zero_crossing_rate, BeatTrackParams, ChromaCqtParams,
    ChromaStftParams, MelSpectrogramParams, MfccParams, OnsetStrengthParams, PyinParams, StftParams,
    TempoParams,
};
use std::process::{Command, Stdio};

pub const SR: u32 = 22_050;
pub const MFCC_DIM: usize = 20;
/// Window for the single-resolution spectral / mfcc / flux features.
pub const SPECTRAL_NFFT: usize = 2048;

/// The resolutions of the mel/chroma stack (fine < mid < coarse). hop = win/4.
/// Trimmed from 4 to 3 — adjacent windows were largely redundant, and dropping
/// one cuts the per-track STFT work without losing the fine/coarse span.
pub const MEL_RESOLUTIONS: [Res; 3] = [
    Res { level: 0, win: 1024, n_mels: 64 },
    Res { level: 1, win: 8192, n_mels: 96 },
    Res { level: 2, win: 32768, n_mels: 128 },
];

#[derive(Debug, Clone, Copy)]
pub struct Res {
    pub level: i64,
    pub win: usize,
    pub n_mels: usize,
}

// ---- similarity axis dims (kept separate; combined with weights at query time) ----
pub const DIM_MEL: usize = 64 + 96 + 128; // 288  mel-stack means (3 resolutions)
pub const DIM_CHROMA: usize = 12 * 3 + 12; // 48  chroma-stack means + cqt5 means
pub const DIM_TONNETZ: usize = 6;
pub const DIM_MFCC: usize = 2 * MFCC_DIM; // 40  mean+std

/// L2 (mean/std) + L1-robust (median/MAD) summary of one time series.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub mean: f32,
    pub std: f32,
    pub median: f32,
    pub mad: f32,
}

/// Per-dimension stats for a matrix family (each row = one band/coeff).
#[derive(Debug, Clone, Default)]
pub struct BandStats {
    pub mean: Vec<f32>,
    pub std: Vec<f32>,
    pub median: Vec<f32>,
    pub mad: Vec<f32>,
}

/// One resolution of a stacked grid feature (mel or chroma).
#[derive(Debug, Clone)]
pub struct GridSummary {
    pub level: i64,
    pub win: usize,
    pub hop: usize,
    pub n_bands: usize,
    pub n_frames: usize,
    pub stats: BandStats,
}

#[derive(Debug, Clone, Default)]
pub struct Spectral {
    pub centroid: Stats,
    pub rolloff: Stats,
    pub bandwidth: Stats,
    pub flatness: Stats,
    pub flux: Stats,
}

#[derive(Debug, Clone)]
pub struct TrackFeatures {
    pub duration_s: f32,
    pub mel_stack: Vec<GridSummary>,
    pub chroma_stack: Vec<GridSummary>,
    pub chroma_cqt5: BandStats, // 12-bin, 5-octave
    pub tonnetz: BandStats,     // 6-dim
    pub mfcc: BandStats,
    pub spectral: Spectral,
    pub rms: Stats,
    pub zcr: Stats,
    // rhythm
    pub tempo_bpm: f32,
    pub beat_bpm: f32,
    pub beat_count: i32,
    pub beat_regularity: f32, // std of inter-beat interval (s); lower = steadier
    // pitch + confidence
    pub pitch_median_hz: f32,
    pub voiced_frac: f32,
    pub voiced_conf: f32, // mean voicing probability
    pub tuning: f32,
    // compression signatures
    pub comp_sig: Vec<u8>, // chroma byte sequence (zstd NCD)
    pub comp_pcm: Vec<u8>, // downsampled i16 PCM (FLAC NCD)
    pub aubio: Option<AubioExtras>,
}

/// aubio-only descriptors rosa lacks. Populated by [`crate::aubio_extra`].
#[derive(Debug, Clone, Default)]
pub struct AubioExtras {
    pub skewness_mean: f32,
    pub skewness_std: f32,
    pub kurtosis_mean: f32,
    pub kurtosis_std: f32,
    pub slope_mean: f32,
    pub slope_std: f32,
    pub decrease_mean: f32,
    pub decrease_std: f32,
    pub onset_rate: f32,
    pub tempo_confidence: f32,
    pub note_count: f32,
    pub note_mean_dur: f32,
}

// ================= decode =================

pub fn decode(path: &str) -> Result<Vec<f64>> {
    match rosa::load(path, Some(SR), true) {
        Ok((sig, _)) if sig.len() > 16 => Ok(scrub(sig)),
        _ => decode_ffmpeg(path).map(scrub),
    }
}

fn decode_ffmpeg(path: &str) -> Result<Vec<f64>> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-i", path, "-ac", "1", "-ar",
               &SR.to_string(), "-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    anyhow::ensure!(out.status.success(), "ffmpeg decode failed for {path}");
    Ok(out.stdout.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f64)
        .collect())
}

fn scrub(mut sig: Vec<f64>) -> Vec<f64> {
    for x in &mut sig {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
    sig
}

/// Central window of `secs` seconds (for the expensive pyin / PCM signature).
fn central(sig: &[f64], secs: f64) -> &[f64] {
    let n = (secs * SR as f64) as usize;
    if sig.len() <= n {
        return sig;
    }
    let start = (sig.len() - n) / 2;
    &sig[start..start + n]
}

// ================= extract =================

/// Featurize a track. `seg` (seconds) restricts the per-frame analysis to a
/// central window — a big speedup over featurizing a whole multi-minute track,
/// with negligible effect on these aggregate statistics. `None` = whole track.
/// `duration_s` always reflects the full track.
pub fn extract(full: &[f64], seg: Option<f32>) -> Result<TrackFeatures> {
    let largest = MEL_RESOLUTIONS[MEL_RESOLUTIONS.len() - 1].win;
    // analysis window: central `seg` seconds, or the whole track
    let sig: &[f64] = match seg {
        Some(s) => central(full, s as f64),
        None => full,
    };
    if sig.len() < largest {
        return Err(anyhow!("signal too short: {} < {}", sig.len(), largest));
    }
    let full_duration = (full.len() as f64 / SR as f64) as f32;
    let sr = SR as f64;

    // --- mel stack (log power) ---
    let mut mel_stack = Vec::with_capacity(4);
    for r in MEL_RESOLUTIONS {
        let hop = r.win / 4;
        let mut mp = MelSpectrogramParams::new(sr);
        mp.stft.n_fft = r.win;
        mp.stft.win_length = r.win;
        mp.stft.hop_length = hop;
        mp.mel = MelFilterParams::new(sr, r.win);
        let mel = mel_spectrogram(sig, &mp);
        mel_stack.push(GridSummary {
            level: r.level, win: r.win, hop, n_bands: r.n_mels, n_frames: mel.cols(),
            stats: band_stats(&mel, r.n_mels, true),
        });
    }

    // --- chroma stack (12-bin, same resolutions) ---
    let mut chroma_stack = Vec::with_capacity(4);
    let mut chroma_mid: Option<rosa::Matrix> = None;
    for r in MEL_RESOLUTIONS {
        let hop = r.win / 4;
        let mut cp = ChromaStftParams::default();
        cp.sr = sr;
        cp.n_fft = r.win;
        cp.win_length = r.win;
        cp.hop_length = hop;
        cp.n_chroma = 12;
        let ch = chroma_stft(sig, &cp);
        chroma_stack.push(GridSummary {
            level: r.level, win: r.win, hop, n_bands: 12, n_frames: ch.cols(),
            stats: band_stats(&ch, 12, false),
        });
        if r.level == 1 {
            chroma_mid = Some(ch);
        }
    }

    // --- 5-octave chroma (register-aware harmony) ---
    let mut ccp = ChromaCqtParams::default();
    ccp.sr = sr;
    ccp.n_octaves = 5;
    ccp.bins_per_octave = 12; // folds to 12-bin chroma anyway; 36 was 3x wasted CQT work
    ccp.n_chroma = 12;
    let cqt5 = chroma_cqt(sig, &ccp);
    let chroma_cqt5 = band_stats(&cqt5, 12, false);

    // --- tonnetz (polytonal / harmonic) ---
    // Reuse the mid-resolution chroma instead of letting tonnetz recompute its
    // own (profiled at ~2.8s/track). Same output, rosa's own API, no reimpl.
    let tn = match chroma_mid.as_ref() {
        Some(c) => tonnetz(None, sr, Some(c)),
        None => tonnetz(Some(sig), sr, None),
    };
    let tonnetz_stats = band_stats(&tn, 6, false);

    // --- timbre: mfcc + spectral shape ---
    let n_fft = SPECTRAL_NFFT;
    let hop = n_fft / 4;
    let mag = spectrogram(sig, &StftParams::new(n_fft), 1.0);
    let freqs = fft_frequencies(sr, n_fft);
    let mfccs = mfcc(&mel_spectrogram(sig, &MelSpectrogramParams::new(sr)), &MfccParams::default());
    let mfcc_stats = band_stats(&mfccs, MFCC_DIM, false);

    let centroid = spectral_centroid(&mag, &freqs);
    let rolloff = spectral_rolloff(&mag, &freqs, 0.85);
    let bandwidth = spectral_bandwidth(&mag, &freqs, &centroid, 2.0, true);
    let flatness = spectral_flatness(&mag, 1e-10, 2.0);
    let flux = dasp_flux(sig, n_fft, hop);
    let spectral = Spectral {
        centroid: stats(&centroid), rolloff: stats(&rolloff), bandwidth: stats(&bandwidth),
        flatness: stats(&flatness), flux: stats(&flux),
    };

    let rms_v = rms(sig, n_fft, hop, true);
    let zcr_v = zero_crossing_rate(sig, n_fft, hop, true);

    // --- rhythm: tempo + beat tracking ---
    let onset_env = onset_strength(sig, &OnsetStrengthParams::new(sr));
    let tempo_bpm = rosa::tempo(&onset_env, &TempoParams::new(sr)) as f32;
    let (beat_bpm, beats) = beat_track(&onset_env, &BeatTrackParams::new(sr));
    let beat_regularity = beat_regularity_s(&beats, hop, sr);

    // --- pitch (pyin) on a central 30s window (pyin is costly) ---
    let pwin = central(sig, 30.0);
    let pres = pyin(pwin, &PyinParams::new(65.0, 2093.0));
    let (pitch_median_hz, voiced_frac, voiced_conf) = pitch_summary(&pres);
    let tuning = estimate_tuning(Some(sig), sr, None, 2048, 0.01, 12) as f32;

    // --- compression signatures ---
    let comp_sig = match &chroma_mid {
        Some(c) => chroma_to_bytes(c, 3000),
        None => Vec::new(),
    };
    let comp_pcm = pcm_signature(central(full, 30.0));

    Ok(TrackFeatures {
        duration_s: full_duration,
        mel_stack,
        chroma_stack,
        chroma_cqt5,
        tonnetz: tonnetz_stats,
        mfcc: mfcc_stats,
        spectral,
        rms: stats(&rms_v),
        zcr: stats(&zcr_v),
        tempo_bpm,
        beat_bpm: beat_bpm as f32,
        beat_count: beats.len() as i32,
        beat_regularity,
        pitch_median_hz,
        voiced_frac,
        voiced_conf,
        tuning,
        comp_sig,
        comp_pcm,
        aubio: crate::aubio_extra::compute(sig),
    })
}

// ================= similarity axis vectors (each L2-normalized) =================

pub fn vec_mel(f: &TrackFeatures) -> Vec<f32> {
    let mut v = Vec::with_capacity(DIM_MEL);
    for g in &f.mel_stack {
        push_n(&mut v, &g.stats.mean, g.n_bands);
    }
    finalize(v, DIM_MEL)
}
pub fn vec_chroma(f: &TrackFeatures) -> Vec<f32> {
    let mut v = Vec::with_capacity(DIM_CHROMA);
    for g in &f.chroma_stack {
        push_n(&mut v, &g.stats.mean, 12);
    }
    push_n(&mut v, &f.chroma_cqt5.mean, 12);
    finalize(v, DIM_CHROMA)
}
pub fn vec_tonnetz(f: &TrackFeatures) -> Vec<f32> {
    finalize(f.tonnetz.mean.clone(), DIM_TONNETZ)
}
pub fn vec_mfcc(f: &TrackFeatures) -> Vec<f32> {
    let mut v = Vec::with_capacity(DIM_MFCC);
    push_n(&mut v, &f.mfcc.mean, MFCC_DIM);
    push_n(&mut v, &f.mfcc.std, MFCC_DIM);
    finalize(v, DIM_MFCC)
}

fn finalize(mut v: Vec<f32>, dim: usize) -> Vec<f32> {
    v.resize(dim, 0.0);
    for x in &mut v {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-9 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

fn push_n(out: &mut Vec<f32>, xs: &[f32], n: usize) {
    for i in 0..n {
        out.push(xs.get(i).copied().unwrap_or(0.0));
    }
}

// ================= helpers =================

fn dasp_flux(sig: &[f64], n_fft: usize, hop: usize) -> Vec<f64> {
    let s32: Vec<f32> = sig.iter().map(|&x| x as f32).collect();
    match dasp_rs::feat::spectral(&s32, SR).n_fft(n_fft).hop_length(hop).spectral_flux() {
        Ok(arr) => arr.iter().map(|&x| x as f64).collect(),
        Err(_) => Vec::new(),
    }
}

fn beat_regularity_s(beats: &[usize], hop: usize, sr: f64) -> f32 {
    if beats.len() < 3 {
        return 0.0;
    }
    let iois: Vec<f64> = beats.windows(2).map(|w| (w[1] - w[0]) as f64 * hop as f64 / sr).collect();
    let m = iois.iter().sum::<f64>() / iois.len() as f64;
    let var = iois.iter().map(|x| (x - m).powi(2)).sum::<f64>() / iois.len() as f64;
    var.sqrt() as f32
}

fn pitch_summary(p: &rosa::PyinResult) -> (f32, f32, f32) {
    let voiced: Vec<f64> = p
        .f0
        .iter()
        .zip(&p.voiced_flag)
        .filter(|(f, &v)| v && f.is_finite())
        .map(|(f, _)| *f)
        .collect();
    let frac = if p.f0.is_empty() { 0.0 } else { voiced.len() as f32 / p.f0.len() as f32 };
    let conf = if p.voiced_prob.is_empty() {
        0.0
    } else {
        (p.voiced_prob.iter().sum::<f64>() / p.voiced_prob.len() as f64) as f32
    };
    let median = if voiced.is_empty() {
        0.0
    } else {
        let mut s = voiced.clone();
        s.sort_by(|a, b| a.total_cmp(b));
        median_sorted(&s) as f32
    };
    (median, frac, conf)
}

fn chroma_to_bytes(chroma: &rosa::Matrix, max_frames: usize) -> Vec<u8> {
    let rows = chroma.rows().min(12);
    let frames = chroma.cols();
    if rows == 0 || frames == 0 {
        return Vec::new();
    }
    let step = (frames / max_frames).max(1);
    let mut out = Vec::new();
    let mut t = 0;
    while t < frames {
        for p in 0..rows {
            out.push((chroma.get(p, t).clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        t += step;
    }
    out
}

/// Downsample to ~4.41 kHz mono i16 little-endian — the object compressed by
/// FLAC for the audio-native NCD axis.
fn pcm_signature(sig: &[f64]) -> Vec<u8> {
    let stride = 5; // 22050 / 5 ≈ 4410 Hz
    let mut out = Vec::with_capacity(sig.len() / stride * 2);
    let mut i = 0;
    while i < sig.len() {
        let s = (sig[i].clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&s.to_le_bytes());
        i += stride;
    }
    out
}

// ---- statistics: L2 (mean/std) + L1-robust (median/MAD) ----

fn stats(xs: &[f64]) -> Stats {
    if xs.is_empty() {
        return Stats::default();
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
    let mut s: Vec<f64> = xs.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let median = median_sorted(&s);
    let mut dev: Vec<f64> = xs.iter().map(|x| (x - median).abs()).collect();
    dev.sort_by(|a, b| a.total_cmp(b));
    let mad = median_sorted(&dev);
    Stats { mean: mean as f32, std: var.sqrt() as f32, median: median as f32, mad: mad as f32 }
}

fn median_sorted(s: &[f64]) -> f64 {
    let n = s.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// Per-row robust+L2 stats of a matrix. `log` applies ln to power values first.
fn band_stats(m: &rosa::Matrix, n: usize, log: bool) -> BandStats {
    let n = n.min(m.rows());
    let mut bs = BandStats {
        mean: Vec::with_capacity(n), std: Vec::with_capacity(n),
        median: Vec::with_capacity(n), mad: Vec::with_capacity(n),
    };
    for r in 0..n {
        let row: Vec<f64> = if log {
            m.row(r).iter().map(|&x| (x + 1e-10).ln()).collect()
        } else {
            m.row(r).to_vec()
        };
        let s = stats(&row);
        bs.mean.push(s.mean);
        bs.std.push(s.std);
        bs.median.push(s.median);
        bs.mad.push(s.mad);
    }
    bs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_on_synthetic_sine() {
        let n = (SR as f64 * 3.0) as usize;
        let sig: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / SR as f64).sin())
            .collect();
        let f = extract(&sig, None).expect("extract");
        assert_eq!(f.mel_stack.len(), 3);
        assert_eq!(f.chroma_stack.len(), 3);
        assert_eq!(f.tonnetz.mean.len(), 6);
        assert_eq!(f.chroma_cqt5.mean.len(), 12);
        assert_eq!(vec_mel(&f).len(), DIM_MEL);
        assert_eq!(vec_chroma(&f).len(), DIM_CHROMA);
        assert_eq!(vec_tonnetz(&f).len(), DIM_TONNETZ);
        assert_eq!(vec_mfcc(&f).len(), DIM_MFCC);
        // robust stats present
        assert_eq!(f.mfcc.median.len(), MFCC_DIM);
        assert!(f.spectral.centroid.median.is_finite());
        assert!(!f.comp_sig.is_empty());
        assert!(!f.comp_pcm.is_empty());
    }

    #[test]
    fn rejects_too_short() {
        assert!(extract(&vec![0.0f64; 1000], None).is_err());
    }
}
