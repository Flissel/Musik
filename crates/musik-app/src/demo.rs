//! Synthetische Tracks für den Fall ohne Musiksammlung.
//!
//! Damit lässt sich die Oberfläche starten und beurteilen, ohne dass Dateien
//! vorliegen müssen — und sie zeigt dabei echte Wellenformen und ein echtes
//! Beatgrid, weil das Material durch dieselbe Analyse läuft wie alles andere.

use std::f32::consts::PI;

use audio_core::Track;

pub struct DemoTrack {
    pub track: Track,
    pub artist: String,
    pub title: String,
}

pub fn deck_a(sample_rate: u32) -> DemoTrack {
    DemoTrack {
        track: bauen(sample_rate, 128.0, 16, muster_a),
        artist: "Demo".into(),
        title: "Vier auf die Eins · 128".into(),
    }
}

pub fn deck_b(sample_rate: u32) -> DemoTrack {
    DemoTrack {
        track: bauen(sample_rate, 124.0, 16, muster_b),
        artist: "Demo".into(),
        title: "Snare-Muster · 124".into(),
    }
}

fn muster_a(bar: usize, step: usize, add: &mut dyn FnMut(usize, f32), rate: u32) {
    if step.is_multiple_of(4) {
        kick(add, rate);
    }
    if step % 4 == 2 {
        hat(add, rate, 0.22);
    }
    if step.is_multiple_of(2) {
        let note = [55.0, 55.0, 73.42, 65.41][bar % 4];
        bass(add, note, rate, 0.30);
    }
}

fn muster_b(bar: usize, step: usize, add: &mut dyn FnMut(usize, f32), rate: u32) {
    if step.is_multiple_of(4) {
        kick(add, rate);
    }
    if step % 8 == 4 {
        snare(add, rate);
    }
    if step % 2 == 1 {
        let note = [49.0, 61.74, 49.0, 58.27][bar % 4];
        bass(add, note, rate, 0.26);
    }
}

/// Ein Muster füllt einen Sechzehntel-Schritt eines Taktes.
///
/// Die Klänge werden über einen Rückruf addiert statt zurückgegeben, damit
/// sich mehrere Klänge im selben Schritt überlagern können.
type Muster = fn(usize, usize, &mut dyn FnMut(usize, f32), u32);

fn bauen(rate: u32, bpm: f32, bars: usize, muster: Muster) -> Track {
    let frames_je_step = rate as f32 * 60.0 / bpm / 4.0;
    let total = (frames_je_step * 16.0 * bars as f32) as usize;
    let mut mono = vec![0.0f32; total];

    for bar in 0..bars {
        for step in 0..16 {
            let start = ((bar * 16 + step) as f32 * frames_je_step) as usize;
            let mut add = |offset: usize, value: f32| {
                let idx = start + offset;
                if idx < total {
                    mono[idx] += value;
                }
            };
            muster(bar, step, &mut add, rate);
        }
    }

    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    let scale = 0.75 / peak;

    Track {
        samples: mono
            .into_iter()
            .flat_map(|v| {
                let s = v * scale;
                [s, s]
            })
            .collect(),
        sample_rate: rate,
    }
}

fn kick(add: &mut dyn FnMut(usize, f32), rate: u32) {
    let n = (rate as f32 * 0.25) as usize;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-t * 22.0).exp();
        let f = 110.0 * (-t * 30.0).exp() + 45.0;
        add(i, (2.0 * PI * f * t).sin() * env * 0.9);
    }
}

fn snare(add: &mut dyn FnMut(usize, f32), rate: u32) {
    let n = (rate as f32 * 0.19) as usize;
    let mut seed = 0x2545_F491u32;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-t * 30.0).exp();
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
        add(
            i,
            (noise * 0.6 + (2.0 * PI * 190.0 * t).sin() * 0.4) * env * 0.6,
        );
    }
}

fn hat(add: &mut dyn FnMut(usize, f32), rate: u32, gain: f32) {
    let n = (rate as f32 * 0.05) as usize;
    let mut seed = 0x9E37_79B9u32;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-t * 120.0).exp();
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
        add(i, noise * env * gain);
    }
}

fn bass(add: &mut dyn FnMut(usize, f32), freq: f32, rate: u32, gain: f32) {
    let n = (rate as f32 * 0.3) as usize;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (1.0 - (-t * 120.0).exp()) * (-t * 6.0).exp();
        let saw = 2.0 * ((freq * t) % 1.0) - 1.0;
        add(i, saw * env * gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Demo verspricht in ihrem Titel ein Tempo. Findet die Analyse es
    /// nicht wieder, zeigt die Oberfläche „— BPM" und Sync bleibt tot — dann
    /// taugt die Demo nicht als Vorführung.
    #[test]
    fn die_analyse_findet_das_versprochene_tempo() {
        for (demo, erwartet) in [(deck_a(48_000), 128.0), (deck_b(48_000), 124.0)] {
            let analyse = analysis::analyze(&demo.track);
            let bpm = analyse
                .bpm
                .unwrap_or_else(|| panic!("{}: kein Tempo erkannt", demo.title));
            assert!(
                (bpm - erwartet).abs() < 1.0,
                "{}: {bpm} statt {erwartet}",
                demo.title
            );
            assert!(
                analyse.beat_anchor_frames.is_some(),
                "{}: kein Anker",
                demo.title
            );
        }
    }
}
