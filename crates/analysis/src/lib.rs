//! Offline-Analyse: Tempo, Beatgrid und Wellenform-Spitzen.
//!
//! Läuft bewusst außerhalb des Abspielpfads. Nichts hier hat Echtzeitauflagen,
//! dafür ist alles reproduzierbar und wird über den Inhalts-Hash
//! zwischengespeichert — siehe [`sidecar`].

pub mod onset;
pub mod peaks;
pub mod sidecar;
pub mod tempo;

#[cfg(test)]
pub(crate) mod testing;

use audio_core::Track;

pub use sidecar::{Analysis, Store};
pub use tempo::Beatgrid;

/// Analysiert einen dekodierten Track vollständig.
pub fn analyze(track: &Track) -> Analysis {
    let envelope = onset::onset_envelope(&track.samples, track.sample_rate);
    let grid = tempo::detect(&envelope)
        .map(|g| tempo::refine_anchor(&track.samples, track.sample_rate, g));
    let levels = peaks::compute(&track.samples);

    Analysis {
        version: sidecar::FORMAT_VERSION,
        fingerprint: sidecar::fingerprint(track),
        sample_rate: track.sample_rate,
        frames: track.frames() as u64,
        duration_secs: track.duration_secs(),
        bpm: grid.map(|g| g.bpm),
        beat_anchor_frames: grid.map(|g| g.anchor_frames),
        bpm_confidence: grid.map(|g| g.confidence),
        peaks: levels
            .iter()
            .map(sidecar::PeakLevelData::from_level)
            .collect(),
    }
}

/// Analysiert einen Track, sofern kein passendes Sidecar vorliegt.
///
/// Gibt zusätzlich zurück, ob gerechnet werden musste (`true`) oder der Cache
/// gereicht hat (`false`).
pub fn analyze_cached(track: &Track, store: &Store) -> (Analysis, bool) {
    let fp = sidecar::fingerprint(track);
    if let Some(vorhanden) = store.load(&fp) {
        return (vorhanden, false);
    }
    (analyze(track), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::click_track;

    fn track(bpm: f64, secs: f64) -> Track {
        Track {
            samples: click_track(bpm, 44_100, secs, 0.0),
            sample_rate: 44_100,
        }
    }

    #[test]
    fn vollstaendige_analyse_liefert_alle_felder() {
        let t = track(128.0, 25.0);
        let a = analyze(&t);

        assert_eq!(a.version, sidecar::FORMAT_VERSION);
        assert_eq!(a.sample_rate, 44_100);
        assert_eq!(a.frames, t.frames() as u64);
        assert!((a.duration_secs - 25.0).abs() < 0.1);
        assert_eq!(a.peaks.len(), peaks::LEVELS.len());

        let bpm = a.bpm.expect("kein BPM erkannt");
        assert!((bpm - 128.0).abs() < 1.0, "BPM {bpm:.2}");
        assert!(a.beat_anchor_frames.is_some());
    }

    #[test]
    fn gleichfoermiges_material_liefert_kein_tempo() {
        // Ein Dauerton hat keine Onsets — hier ist `None` die richtige Antwort,
        // nicht eine geratene Zahl.
        let frames = 44_100 * 20;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let v = (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 44_100.0).sin() * 0.5;
            samples.push(v);
            samples.push(v);
        }
        let t = Track {
            samples,
            sample_rate: 44_100,
        };

        let a = analyze(&t);
        assert!(
            a.bpm.is_none(),
            "Tempo aus einem Dauerton geraten: {:?}",
            a.bpm
        );
    }

    #[test]
    fn zweiter_lauf_kommt_aus_dem_cache() {
        let dir = std::env::temp_dir().join("musik-analysis-test-cached");
        std::fs::remove_dir_all(&dir).ok();
        let store = Store::new(&dir);

        let t = track(128.0, 25.0);

        let (erste, gerechnet) = analyze_cached(&t, &store);
        assert!(gerechnet);
        store.save(&erste).expect("speichern");

        let (zweite, nochmal_gerechnet) = analyze_cached(&t, &store);
        assert!(!nochmal_gerechnet, "Cache wurde nicht genutzt");
        assert_eq!(zweite.fingerprint, erste.fingerprint);
        assert_eq!(zweite.bpm, erste.bpm);

        std::fs::remove_dir_all(&dir).ok();
    }
}
