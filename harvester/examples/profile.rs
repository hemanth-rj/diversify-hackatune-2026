//! Per-stage timing of the feature pipeline on one real track.
//! `cargo run --release --example profile -- corpus/audio/1000095.mp3`
//!
//! Mirrors features::extract's calls (full track, no segment) and prints ms per
//! stage, so we can see whether a shared-STFT refactor would actually help.

use harvester::features::{decode, MEL_RESOLUTIONS, SPECTRAL_NFFT, SR};
use rosa::stft::MelFilterParams;
use rosa::{
    beat_track, chroma_cqt, chroma_stft, estimate_tuning, fft_frequencies, mel_spectrogram, mfcc,
    onset_strength, pyin, rms, spectral_bandwidth, spectral_centroid, spectral_flatness,
    spectral_rolloff, spectrogram, tonnetz, zero_crossing_rate, BeatTrackParams, ChromaCqtParams,
    ChromaStftParams, MelSpectrogramParams, MfccParams, OnsetStrengthParams, PyinParams, StftParams,
    TempoParams,
};
use std::time::Instant;

fn t<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let s = Instant::now();
    let r = f();
    println!("  {:<22} {:>7.1} ms", label, s.elapsed().as_secs_f64() * 1000.0);
    r
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: profile <mp3>");
    let sig = decode(&path).expect("decode");
    let sr = SR as f64;
    println!("decoded {} samples ({:.1}s)\n--- stage timings (full track) ---", sig.len(), sig.len() as f64 / sr);

    let whole = Instant::now();

    for r in MEL_RESOLUTIONS {
        t(&format!("mel win={}", r.win), || {
            let mut mp = MelSpectrogramParams::new(sr);
            mp.stft.n_fft = r.win; mp.stft.win_length = r.win; mp.stft.hop_length = r.win / 4;
            mp.mel = MelFilterParams::new(sr, r.win);
            mel_spectrogram(&sig, &mp)
        });
    }
    for r in MEL_RESOLUTIONS {
        t(&format!("chroma win={}", r.win), || {
            let mut cp = ChromaStftParams::default();
            cp.sr = sr; cp.n_fft = r.win; cp.win_length = r.win; cp.hop_length = r.win / 4; cp.n_chroma = 12;
            chroma_stft(&sig, &cp)
        });
    }
    t("chroma_cqt5(12bpo)", || {
        let mut ccp = ChromaCqtParams::default();
        ccp.sr = sr; ccp.n_octaves = 5; ccp.bins_per_octave = 12; ccp.n_chroma = 12;
        chroma_cqt(&sig, &ccp)
    });
    let chroma_mid = {
        let mut cp = ChromaStftParams::default();
        cp.sr = sr; cp.n_fft = 8192; cp.win_length = 8192; cp.hop_length = 2048; cp.n_chroma = 12;
        chroma_stft(&sig, &cp)
    };
    t("tonnetz(reuse chroma)", || tonnetz(None, sr, Some(&chroma_mid)));

    let mag = t("spectrogram(2048)", || spectrogram(&sig, &StftParams::new(SPECTRAL_NFFT), 1.0));
    let freqs = fft_frequencies(sr, SPECTRAL_NFFT);
    t("mfcc(+mel)", || mfcc(&mel_spectrogram(&sig, &MelSpectrogramParams::new(sr)), &MfccParams::default()));
    let centroid = t("spec.centroid", || spectral_centroid(&mag, &freqs));
    t("spec.rolloff", || spectral_rolloff(&mag, &freqs, 0.85));
    t("spec.bandwidth", || spectral_bandwidth(&mag, &freqs, &centroid, 2.0, true));
    t("spec.flatness", || spectral_flatness(&mag, 1e-10, 2.0));
    t("dasp.flux", || {
        let s32: Vec<f32> = sig.iter().map(|&x| x as f32).collect();
        dasp_rs::feat::spectral(&s32, SR).n_fft(SPECTRAL_NFFT).hop_length(SPECTRAL_NFFT / 4).spectral_flux().ok();
    });
    t("rms", || rms(&sig, SPECTRAL_NFFT, SPECTRAL_NFFT / 4, true));
    t("zcr", || zero_crossing_rate(&sig, SPECTRAL_NFFT, SPECTRAL_NFFT / 4, true));
    let onset = t("onset_strength", || onset_strength(&sig, &OnsetStrengthParams::new(sr)));
    t("tempo", || rosa::tempo(&onset, &TempoParams::new(sr)));
    t("beat_track", || beat_track(&onset, &BeatTrackParams::new(sr)));
    t("tuning", || estimate_tuning(Some(&sig), sr, None, 2048, 0.01, 12));
    // pyin on central 30s (as in extract)
    let pwin = {
        let n = (30.0 * sr) as usize;
        if sig.len() <= n { &sig[..] } else { let s = (sig.len() - n) / 2; &sig[s..s + n] }
    };
    t("pyin(30s)", || pyin(pwin, &PyinParams::new(65.0, 2093.0)));
    t("aubio_extra(all)", || harvester::aubio_extra::compute(&sig));

    println!("--- per-stage TOTAL {:.1} ms ---", whole.elapsed().as_secs_f64() * 1000.0);

    // the real thing, end to end (3 runs) — reflects every change in extract()
    for i in 0..3 {
        t(&format!("extract() run {i}"), || harvester::features::extract(&sig, None).unwrap());
    }
}
