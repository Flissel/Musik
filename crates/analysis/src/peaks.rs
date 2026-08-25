//! Vorberechnete Wellenform-Spitzen in mehreren Auflösungen.
//!
//! Die Darstellung darf nie über den Rohdaten rechnen: Bei fünf Minuten Audio
//! sind das dreizehn Millionen Werte pro Deck und Bild. Stattdessen werden
//! Min/Max je Fenster einmalig abgelegt, in drei Stufen von Übersicht bis Zoom.
//!
//! Quantisiert auf `i8` — für eine Wellenformdarstellung sind 8 Bit reichlich,
//! und es viertelt die Dateigröße gegenüber `f32`.

use audio_core::track::CHANNELS;

/// Samples pro Spitze, von fein nach grob.
pub const LEVELS: [u32; 3] = [256, 2048, 16384];

#[derive(Debug, Clone)]
pub struct PeakLevel {
    pub samples_per_peak: u32,
    pub min: Vec<i8>,
    pub max: Vec<i8>,
}

impl PeakLevel {
    pub fn len(&self) -> usize {
        self.min.len()
    }

    pub fn is_empty(&self) -> bool {
        self.min.is_empty()
    }
}

/// Berechnet alle Stufen aus interleaved Stereo.
pub fn compute(samples: &[f32]) -> Vec<PeakLevel> {
    let mono: Vec<f32> = samples
        .as_chunks::<CHANNELS>()
        .0
        .iter()
        .map(|f| 0.5 * (f[0] + f[1]))
        .collect();

    LEVELS.iter().map(|n| level(&mono, *n)).collect()
}

fn level(mono: &[f32], samples_per_peak: u32) -> PeakLevel {
    let bucket = samples_per_peak as usize;
    let count = mono.len().div_ceil(bucket);

    let mut min = Vec::with_capacity(count);
    let mut max = Vec::with_capacity(count);

    for chunk in mono.chunks(bucket) {
        let (lo, hi) = chunk
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        min.push(quantize(lo));
        max.push(quantize(hi));
    }

    PeakLevel {
        samples_per_peak,
        min,
        max,
    }
}

fn quantize(v: f32) -> i8 {
    (v.clamp(-1.0, 1.0) * 127.0).round() as i8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spitzen_treffen_die_extremwerte() {
        // Rampe von -1 bis +1 über 4096 Frames.
        let frames = 4096;
        let mut samples = Vec::with_capacity(frames * CHANNELS);
        for i in 0..frames {
            let v = -1.0 + 2.0 * i as f32 / (frames - 1) as f32;
            samples.push(v);
            samples.push(v);
        }

        let levels = compute(&samples);
        assert_eq!(levels.len(), LEVELS.len());

        let fein = &levels[0];
        assert_eq!(fein.samples_per_peak, 256);
        assert_eq!(fein.len(), frames / 256);

        // Erster Bucket beginnt bei -1, letzter endet bei +1.
        assert_eq!(fein.min[0], -127);
        assert_eq!(*fein.max.last().unwrap(), 127);

        // Innerhalb eines Buckets muss max >= min gelten.
        for (lo, hi) in fein.min.iter().zip(&fein.max) {
            assert!(hi >= lo);
        }
    }

    #[test]
    fn groebere_stufen_haben_weniger_spitzen() {
        let samples = vec![0.5f32; 100_000 * CHANNELS];
        let levels = compute(&samples);

        for pair in levels.windows(2) {
            assert!(
                pair[0].len() > pair[1].len(),
                "Stufen sind nicht monoton gröber"
            );
        }
    }

    #[test]
    fn leeres_material_ergibt_leere_stufen() {
        let levels = compute(&[]);
        assert!(levels.iter().all(|l| l.is_empty()));
    }

    #[test]
    fn uebersteuerung_wird_geklemmt() {
        let samples = vec![4.0f32; 512 * CHANNELS];
        let levels = compute(&samples);
        assert_eq!(levels[0].max[0], 127);
    }
}
