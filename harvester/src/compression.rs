//! Normalized Compression Distance (NCD) — a practical, parameter-free
//! approximation of the Kolmogorov / information distance.
//!
//! Kolmogorov complexity K(x) (the length of the shortest program that outputs
//! x) is uncomputable, so we approximate K(x) by C(x), the size of x under a
//! real compressor (zstd). The normalized information distance becomes:
//!
//!     NCD(x, y) = ( C(xy) - min(C(x), C(y)) ) / max(C(x), C(y))
//!
//! ~0 when x and y share structure (concatenating them compresses almost as well
//! as one alone), ~1 when they're unrelated. It's a genuinely different axis
//! from the feature-vector cosine: it sees repeated/sequential structure in the
//! chroma stream, not a fixed summary. We use it to re-rank a vector shortlist.

use std::io::Write;
use std::process::{Command, Stdio};

const LEVEL: i32 = 19;

/// Compressed length of `x` under zstd — a general-purpose stand-in for K(x),
/// applied to the quantized chroma sequence (harmonic structure).
pub fn clen(x: &[u8]) -> usize {
    if x.is_empty() {
        return 0;
    }
    zstd::encode_all(x, LEVEL).map(|c| c.len()).unwrap_or(x.len())
}

/// Compressed length under **FLAC** (an audio-native lossless codec) — the
/// stand-in for K(x) on the raw PCM signature. FLAC's linear prediction models
/// audio redundancy a general compressor can't, so NCD over FLAC captures
/// waveform-level structural similarity. Encoded via ffmpeg (always on the box).
pub fn flac_len(pcm_s16le: &[u8]) -> usize {
    if pcm_s16le.is_empty() {
        return 0;
    }
    flac_encode(pcm_s16le).map(|b| b.len()).unwrap_or(pcm_s16le.len())
}

fn flac_encode(pcm: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error",
            "-f", "s16le", "-ar", "4410", "-ac", "1", "-i", "pipe:0",
            "-f", "flac", "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("stdin");
    let data = pcm.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child.wait_with_output()?;
    let _ = writer.join();
    Ok(out.stdout)
}

/// NCD using the FLAC compressor over PCM signatures (audio-native axis).
pub fn ncd_flac(x: &[u8], y: &[u8], fx: usize, fy: usize) -> f32 {
    if x.is_empty() || y.is_empty() {
        return 1.0;
    }
    let mut xy = Vec::with_capacity(x.len() + y.len());
    xy.extend_from_slice(x);
    xy.extend_from_slice(y);
    let fxy = flac_len(&xy);
    let lo = fx.min(fy);
    let hi = fx.max(fy);
    if hi == 0 {
        return 1.0;
    }
    (fxy.saturating_sub(lo) as f32 / hi as f32).clamp(0.0, 1.5)
}

/// NCD(x, y) in roughly [0, 1] (0 = structurally identical). Pass the
/// precomputed C(x), C(y) so a shortlist re-rank doesn't recompress the seed and
/// each candidate from scratch.
pub fn ncd(x: &[u8], y: &[u8], cx: usize, cy: usize) -> f32 {
    if x.is_empty() || y.is_empty() {
        return 1.0;
    }
    let mut xy = Vec::with_capacity(x.len() + y.len());
    xy.extend_from_slice(x);
    xy.extend_from_slice(y);
    let cxy = clen(&xy);
    let lo = cx.min(cy);
    let hi = cx.max(cy);
    if hi == 0 {
        return 1.0;
    }
    (cxy.saturating_sub(lo) as f32 / hi as f32).clamp(0.0, 1.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_near_zero_unrelated_is_higher() {
        let a: Vec<u8> = (0..2000).map(|i| (i % 12) as u8).collect(); // periodic
        let b = a.clone();
        let c: Vec<u8> = (0..2000).map(|i| ((i * 7 + 3) % 251) as u8).collect(); // noisy
        let (ca, cb, cc) = (clen(&a), clen(&b), clen(&c));
        let same = ncd(&a, &b, ca, cb);
        let diff = ncd(&a, &c, ca, cc);
        assert!(same < diff, "identical NCD {same} should be < unrelated NCD {diff}");
        assert!(same < 0.5, "identical NCD should be small, got {same}");
    }
}
