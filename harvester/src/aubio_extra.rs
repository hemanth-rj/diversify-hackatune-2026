//! aubio-only descriptors that `rosa` does not provide: higher-order spectral
//! moments (skewness, kurtosis, slope, decrease), an onset rate, and a tempo
//! *confidence*. Always built (aubio is a hard dependency); the GCC>=14 CFLAGS
//! workaround lives in `.cargo/config.toml`.

use crate::features::AubioExtras;
use aubio::{Notes, Onset, OnsetMode, PVoc, SpecDesc, SpecShape, Tempo};

const WIN: usize = 2048;
const HOP: usize = 512;

/// Compute aubio's higher-order descriptors over the (f64) signal. Returns None
/// only if the signal is too short or aubio fails to initialise.
pub fn compute(sig: &[f64]) -> Option<AubioExtras> {
    let sr = crate::features::SR;
    if sig.len() < WIN {
        return None;
    }
    let sig: Vec<f32> = sig.iter().map(|&x| x as f32).collect();

    let mut pvoc = PVoc::new(WIN, HOP).ok()?;
    let mut grain = vec![0f32; WIN + 2];
    let mut sd_skew = SpecDesc::new(SpecShape::Skewness, WIN).ok()?;
    let mut sd_kurt = SpecDesc::new(SpecShape::Kurtosis, WIN).ok()?;
    let mut sd_slope = SpecDesc::new(SpecShape::Slope, WIN).ok()?;
    let mut sd_decr = SpecDesc::new(SpecShape::Decrease, WIN).ok()?;
    let mut onset = Onset::new(OnsetMode::Hfc, WIN, HOP, sr).ok()?;
    let mut tempo = Tempo::new(OnsetMode::SpecFlux, WIN, HOP, sr).ok()?;
    let mut notes = Notes::new(WIN, HOP, sr).ok()?;

    let mut skew = Vec::new();
    let mut kurt = Vec::new();
    let mut slope = Vec::new();
    let mut decr = Vec::new();
    let mut onset_count = 0usize;
    let mut note_count = 0usize;
    let mut note_durs: Vec<f32> = Vec::new();
    let mut cur_onset_frame: Option<usize> = None;
    let mut oout = [0f32; 1];
    let mut tout = [0f32; 1];
    let secs_per_hop = HOP as f32 / sr as f32;

    for (fi, chunk) in sig.chunks(HOP).enumerate() {
        if chunk.len() < HOP {
            break;
        }
        if pvoc.do_(chunk, grain.as_mut_slice()).is_err() {
            continue;
        }
        skew.push(specdesc(&mut sd_skew, &grain));
        kurt.push(specdesc(&mut sd_kurt, &grain));
        slope.push(specdesc(&mut sd_slope, &grain));
        decr.push(specdesc(&mut sd_decr, &grain));
        if onset.do_(chunk, oout.as_mut_slice()).is_ok() && oout[0] > 0.0 {
            onset_count += 1;
        }
        let _ = tempo.do_(chunk, tout.as_mut_slice());
        // note events: pitch>0 = onset, pitch==0 = offset of the current note
        if let Ok(evs) = notes.do_result(chunk) {
            for ev in evs {
                if ev.pitch > 0.0 {
                    note_count += 1;
                    cur_onset_frame = Some(fi);
                } else if let Some(on) = cur_onset_frame.take() {
                    note_durs.push((fi - on) as f32 * secs_per_hop);
                }
            }
        }
    }

    let dur = sig.len() as f32 / sr as f32;
    let note_mean_dur = if note_durs.is_empty() {
        0.0
    } else {
        note_durs.iter().sum::<f32>() / note_durs.len() as f32
    };
    let (sk_m, sk_s) = mean_std(&skew);
    let (ku_m, ku_s) = mean_std(&kurt);
    let (sl_m, sl_s) = mean_std(&slope);
    let (de_m, de_s) = mean_std(&decr);
    Some(AubioExtras {
        skewness_mean: sk_m, skewness_std: sk_s,
        kurtosis_mean: ku_m, kurtosis_std: ku_s,
        slope_mean: sl_m, slope_std: sl_s,
        decrease_mean: de_m, decrease_std: de_s,
        onset_rate: if dur > 0.0 { onset_count as f32 / dur } else { 0.0 },
        tempo_confidence: tempo.get_confidence(),
        note_count: note_count as f32,
        note_mean_dur,
    })
}

/// aubio-rs `do_result` returns Result<Smpl>; treat errors as 0.
fn specdesc(sd: &mut SpecDesc, grain: &[f32]) -> f32 {
    sd.do_result(grain).unwrap_or(0.0)
}

fn mean_std(xs: &[f32]) -> (f32, f32) {
    if xs.is_empty() {
        return (0.0, 0.0);
    }
    let m = xs.iter().sum::<f32>() / xs.len() as f32;
    let v = xs.iter().map(|x| (x - m).powi(2)).sum::<f32>() / xs.len() as f32;
    (m, v.sqrt())
}
