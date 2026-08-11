//! Testsignale für die Analyse.

use audio_core::track::CHANNELS;

/// Erzeugt einen Klick-Track als interleaved Stereo.
///
/// Die Klicks sind kurze, deterministisch erzeugte Rauschstöße mit
/// exponentiellem Abfall — breitbandig genug, damit der spektrale Fluss sie
/// deutlich sieht, und ohne Abhängigkeit auf einen Zufallsgenerator.
pub fn click_track(bpm: f64, sample_rate: u32, secs: f64, offset_secs: f64) -> Vec<f32> {
    let frames = (sample_rate as f64 * secs) as usize;
    let mut out = vec![0.0f32; frames * CHANNELS];

    let period = 60.0 / bpm * sample_rate as f64;
    let burst = ((sample_rate as f64 * 0.008) as usize).max(1);
    let decay = burst as f32 * 0.25;

    let mut seed = 0x1234_5678u32;
    let mut at = offset_secs * sample_rate as f64;

    while (at as usize) < frames {
        let start = at as usize;
        for i in 0..burst {
            let idx = start + i;
            if idx >= frames {
                break;
            }
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
            let env = (-(i as f32) / decay).exp();
            let v = noise * env * 0.8;
            out[idx * CHANNELS] += v;
            out[idx * CHANNELS + 1] += v;
        }
        at += period;
    }

    out
}
