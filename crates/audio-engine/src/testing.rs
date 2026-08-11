//! Testsignale und Messhilfen für die Engine.

pub fn sine(freq: f32, sample_rate: u32, secs: f32) -> Vec<f32> {
    let n = (sample_rate as f32 * secs) as usize;
    let step = 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
    (0..n).map(|i| (step * i as f32).sin()).collect()
}

/// Interleaved Stereo, beide Kanäle gleich.
pub fn sine_stereo(freq: f32, sample_rate: u32, secs: f32) -> Vec<f32> {
    sine(freq, sample_rate, secs)
        .into_iter()
        .flat_map(|v| [v, v])
        .collect()
}

pub fn rms(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    (values.iter().map(|v| (v * v) as f64).sum::<f64>() / values.len() as f64).sqrt() as f32
}

/// Effektivwert eines Kanals aus interleaved Stereo.
pub fn rms_channel(interleaved: &[f32], channel: usize, channels: usize) -> f32 {
    let values: Vec<f32> = interleaved
        .iter()
        .skip(channel)
        .step_by(channels)
        .copied()
        .collect();
    rms(&values)
}

pub fn peak(values: &[f32]) -> f32 {
    values.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}
