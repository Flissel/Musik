//! Onset-Hüllkurve über spektralen Fluss.
//!
//! Grundlage für alles Rhythmische: Statt im Zeitsignal nach lauten Stellen zu
//! suchen, wird gemessen, wie stark sich das *Spektrum* von Frame zu Frame
//! ändert. Ein Hi-Hat auf einer laufenden Bassline erzeugt kaum Pegelanstieg,
//! aber deutlichen spektralen Fluss — deshalb funktioniert das dort, wo eine
//! Pegelmessung versagt.

use rustfft::{num_complex::Complex, FftPlanner};

use audio_core::track::CHANNELS;

pub const WINDOW: usize = 1024;
pub const HOP: usize = 256;

pub struct OnsetEnvelope {
    /// Ein Wert pro Analyse-Frame, halbwellengleichgerichtet, Maximum auf 1.0.
    pub values: Vec<f32>,
    /// Frames pro Sekunde.
    pub rate: f64,
    pub hop: usize,
}

impl OnsetEnvelope {
    /// Rechnet eine Frame-Position (auch gebrochen) in Sample-Frames um.
    pub fn to_sample_frames(&self, frame: f64) -> f64 {
        frame * self.hop as f64
    }
}

/// Berechnet die Hüllkurve aus interleaved Stereo.
pub fn onset_envelope(samples: &[f32], sample_rate: u32) -> OnsetEnvelope {
    let rate = sample_rate as f64 / HOP as f64;
    let mono = downmix(samples);

    if mono.len() < WINDOW {
        return OnsetEnvelope {
            values: Vec::new(),
            rate,
            hop: HOP,
        };
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(WINDOW);
    let window = hann(WINDOW);
    let bins = WINDOW / 2 + 1;

    let mut prev = vec![0.0f32; bins];
    let mut cur = vec![0.0f32; bins];
    let mut scratch = vec![Complex::new(0.0f32, 0.0f32); WINDOW];
    let mut flux = Vec::with_capacity((mono.len() - WINDOW) / HOP + 1);

    let frames = (mono.len() - WINDOW) / HOP + 1;
    for f in 0..frames {
        let off = f * HOP;
        for i in 0..WINDOW {
            scratch[i] = Complex::new(mono[off + i] * window[i], 0.0);
        }
        fft.process(&mut scratch);

        for (k, slot) in cur.iter_mut().enumerate() {
            *slot = scratch[k].norm();
        }

        if f == 0 {
            flux.push(0.0);
        } else {
            // Nur Zuwachs zählt — abklingende Energie ist kein Onset.
            let mut sum = 0.0;
            for k in 0..bins {
                let d = cur[k] - prev[k];
                if d > 0.0 {
                    sum += d;
                }
            }
            flux.push(sum);
        }

        std::mem::swap(&mut prev, &mut cur);
    }

    detrend(&mut flux, rate);

    OnsetEnvelope {
        values: flux,
        rate,
        hop: HOP,
    }
}

fn downmix(samples: &[f32]) -> Vec<f32> {
    samples
        .as_chunks::<CHANNELS>()
        .0
        .iter()
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect()
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos())
        .collect()
}

/// Zieht den gleitenden Mittelwert ab und richtet gleich.
///
/// Ohne diesen Schritt dominiert der laute Teil eines Tracks die gesamte
/// Hüllkurve, und eine leise Passage liefert keine verwertbaren Onsets mehr.
fn detrend(values: &mut [f32], rate: f64) {
    let win = ((0.3 * rate) as usize).max(1) | 1;
    let half = win / 2;
    let n = values.len();
    if n == 0 {
        return;
    }

    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0f64);
    for v in values.iter() {
        prefix.push(prefix[prefix.len() - 1] + *v as f64);
    }

    let mut out = Vec::with_capacity(n);
    for (i, value) in values.iter().enumerate() {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(n);
        let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
        out.push((*value as f64 - mean).max(0.0) as f32);
    }

    let peak = out.iter().cloned().fold(0.0f32, f32::max);
    let scale = if peak > 0.0 { 1.0 / peak } else { 0.0 };
    for (dst, src) in values.iter_mut().zip(out) {
        *dst = src * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::click_track;

    #[test]
    fn klicks_erzeugen_klare_spitzen() {
        let rate = 44_100;
        let track = click_track(120.0, rate, 6.0, 0.0);
        let env = onset_envelope(&track, rate);

        assert!(!env.values.is_empty());
        assert!((env.rate - rate as f64 / HOP as f64).abs() < 1e-9);

        // Bei 120 BPM in 6 s liegen 12 Klicks. Deutlich über der Hälfte des
        // Maximums sollten grob so viele Frames liegen — nicht hunderte.
        let laut = env.values.iter().filter(|v| **v > 0.5).count();
        assert!(
            (1..=60).contains(&laut),
            "unplausibel viele/wenige Spitzen: {laut}"
        );
    }

    #[test]
    fn stille_liefert_keine_onsets() {
        let rate = 44_100;
        let stille = vec![0.0f32; rate as usize * CHANNELS];
        let env = onset_envelope(&stille, rate);

        assert!(env.values.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn zu_kurzes_material_paniert_nicht() {
        let env = onset_envelope(&[0.0; 32], 44_100);
        assert!(env.values.is_empty());
    }
}
