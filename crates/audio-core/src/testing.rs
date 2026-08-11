//! Hilfsmittel für die Tests des Audio-Kerns.

use crate::track::CHANNELS;

/// Erzeugt einen Sinus als interleaved Stereo.
pub fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
    let frames = (rate as f32 * secs) as usize;
    let mut out = vec![0.0; frames * CHANNELS];
    let step = 2.0 * std::f32::consts::PI * freq / rate as f32;

    for i in 0..frames {
        let v = (step * i as f32).sin();
        out[i * CHANNELS] = v;
        out[i * CHANNELS + 1] = v;
    }
    out
}

/// Schätzt die Grundfrequenz über positive Nulldurchgänge des linken Kanals.
///
/// Für einen reinen Sinus genau genug — und anders als eine FFT ohne
/// zusätzliche Abhängigkeit zu haben.
pub fn dominant_freq(samples: &[f32], rate: u32) -> f32 {
    let left: Vec<f32> = samples.iter().step_by(CHANNELS).copied().collect();
    if left.len() < 2 {
        return 0.0;
    }

    let mut cycles = 0u32;
    for w in left.windows(2) {
        if w[0] <= 0.0 && w[1] > 0.0 {
            cycles += 1;
        }
    }

    cycles as f32 * rate as f32 / left.len() as f32
}
