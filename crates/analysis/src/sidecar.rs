//! Ablage der Analyseergebnisse, adressiert über den Inhalt.
//!
//! Der Schlüssel ist ein Hash über die dekodierten Audiodaten — **nicht** über
//! den Dateipfad. Sonst ist die Arbeit weg, sobald jemand einen Ordner
//! umbenennt, und doppelt vorhandene Tracks würden zweimal analysiert.
//!
//! Gehasht wird auf `i16` quantisiert. Ein geänderter ID3-Tag lässt die
//! Sidecars damit unberührt, und die Empfindlichkeit gegenüber winzigen
//! Fließkomma-Unterschieden sinkt deutlich.
//!
//! Sie verschwindet aber nicht: Werte direkt an einer Rundungsgrenze können
//! trotzdem kippen. Ein Decoder-Wechsel kann Sidecars also entwerten — das ist
//! hingenommen, weil die Analyse dann schlicht neu läuft.

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use audio_core::Track;

use crate::peaks::PeakLevel;

/// Bei einer Änderung des Formats hochzählen — ältere Sidecars werden dann
/// verworfen statt falsch gelesen.
///
/// 2: Tonart dazugekommen. Ein Sidecar aus Version 1 hätte sie schlicht nicht,
/// und `serde(default)` würde daraus ein „keine Tonart erkannt" machen — also
/// eine Falschaussage statt einer fehlenden Angabe. Deshalb der Bump: Der
/// Track wird einmal neu gerechnet und weiß danach Bescheid.
pub const FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    pub version: u32,
    pub fingerprint: String,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration_secs: f64,

    /// `None`, wenn kein verlässliches Tempo gefunden wurde.
    #[serde(default)]
    pub bpm: Option<f32>,
    /// Erster Beat in Sample-Frames.
    #[serde(default)]
    pub beat_anchor_frames: Option<u64>,
    #[serde(default)]
    pub bpm_confidence: Option<f32>,

    /// Tonart in üblicher Schreibweise (`Am`, `F#`). `None`, wenn das Material
    /// keine klare hergibt — Perkussion etwa.
    #[serde(default)]
    pub musical_key: Option<String>,
    #[serde(default)]
    pub key_confidence: Option<f32>,

    pub peaks: Vec<PeakLevelData>,
}

impl Analysis {
    /// Die erkannte Tonart als Wert, sofern eine dasteht.
    pub fn tonart(&self) -> Option<audio_core::Tonart> {
        self.musical_key
            .as_deref()
            .and_then(audio_core::Tonart::parse)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeakLevelData {
    pub samples_per_peak: u32,
    /// base64, ein Byte je Spitze (i8 als u8 kodiert).
    pub min: String,
    pub max: String,
}

impl PeakLevelData {
    pub fn from_level(level: &PeakLevel) -> Self {
        PeakLevelData {
            samples_per_peak: level.samples_per_peak,
            min: encode(&level.min),
            max: encode(&level.max),
        }
    }

    pub fn to_level(&self) -> Option<PeakLevel> {
        Some(PeakLevel {
            samples_per_peak: self.samples_per_peak,
            min: decode(&self.min)?,
            max: decode(&self.max)?,
        })
    }
}

fn encode(values: &[i8]) -> String {
    let bytes: Vec<u8> = values.iter().map(|v| *v as u8).collect();
    STANDARD.encode(bytes)
}

fn decode(s: &str) -> Option<Vec<i8>> {
    STANDARD
        .decode(s)
        .ok()
        .map(|b| b.into_iter().map(|v| v as i8).collect())
}

/// Inhalts-Hash eines dekodierten Tracks.
pub fn fingerprint(track: &Track) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&track.sample_rate.to_le_bytes());

    let mut buf = Vec::with_capacity(2 * 8192);
    for chunk in track.samples.chunks(8192) {
        buf.clear();
        for s in chunk {
            let q = (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
            buf.extend_from_slice(&q.to_le_bytes());
        }
        hasher.update(&buf);
    }

    hasher.finalize().to_hex()[..32].to_string()
}

/// Verzeichnisbasierter Speicher für Sidecars.
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    pub fn path_for(&self, fingerprint: &str) -> PathBuf {
        self.root.join(format!("{fingerprint}.json"))
    }

    /// Lädt ein Sidecar. `None` bei fehlender Datei, kaputtem JSON oder
    /// veralteter Formatversion — in allen drei Fällen ist neu rechnen richtig.
    pub fn load(&self, fingerprint: &str) -> Option<Analysis> {
        let raw = std::fs::read_to_string(self.path_for(fingerprint)).ok()?;
        let analysis: Analysis = serde_json::from_str(&raw).ok()?;
        if analysis.version != FORMAT_VERSION {
            return None;
        }
        Some(analysis)
    }

    pub fn save(&self, analysis: &Analysis) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.path_for(&analysis.fingerprint);
        let json = serde_json::to_string_pretty(analysis)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, json)?;
        Ok(path)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peaks;
    use crate::testing::click_track;

    fn track(bpm: f64, secs: f64) -> Track {
        Track {
            samples: click_track(bpm, 44_100, secs, 0.0),
            sample_rate: 44_100,
        }
    }

    #[test]
    fn fingerabdruck_ist_stabil_und_unterscheidet() {
        let a = track(128.0, 2.0);
        let b = track(128.0, 2.0);
        let c = track(140.0, 2.0);

        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&c));
        assert_eq!(fingerprint(&a).len(), 32);
    }

    #[test]
    fn lautstaerkeaenderung_aendert_den_hash() {
        // Der Hash beschreibt den Inhalt, nicht die Datei — anderer Klang,
        // anderer Schlüssel, damit die Analyse neu läuft.
        let a = track(128.0, 2.0);
        let mut b = track(128.0, 2.0);
        for s in b.samples.iter_mut() {
            *s *= 0.5;
        }

        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn spitzen_ueberleben_die_kodierung() {
        let t = track(128.0, 2.0);
        let levels = peaks::compute(&t.samples);

        for original in &levels {
            let zurueck = PeakLevelData::from_level(original)
                .to_level()
                .expect("dekodieren");
            assert_eq!(zurueck.samples_per_peak, original.samples_per_peak);
            assert_eq!(zurueck.min, original.min);
            assert_eq!(zurueck.max, original.max);
        }
    }

    #[test]
    fn speichern_und_laden() {
        let dir = std::env::temp_dir().join("musik-analysis-test-store");
        std::fs::remove_dir_all(&dir).ok();
        let store = Store::new(&dir);

        let analysis = Analysis {
            version: FORMAT_VERSION,
            fingerprint: "abc123".into(),
            sample_rate: 44_100,
            frames: 1000,
            duration_secs: 0.0227,
            bpm: Some(128.0),
            beat_anchor_frames: Some(42),
            bpm_confidence: Some(0.9),
            musical_key: Some("Am".into()),
            key_confidence: Some(0.2),
            peaks: vec![],
        };

        store.save(&analysis).expect("speichern");
        let geladen = store.load("abc123").expect("laden");

        assert_eq!(geladen.bpm, Some(128.0));
        assert_eq!(geladen.beat_anchor_frames, Some(42));
        assert_eq!(geladen.tonart(), audio_core::Tonart::parse("Am"));
        assert!(store.load("gibtesnicht").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn veraltete_version_wird_verworfen() {
        let dir = std::env::temp_dir().join("musik-analysis-test-version");
        std::fs::remove_dir_all(&dir).ok();
        let store = Store::new(&dir);

        let mut analysis = Analysis {
            version: FORMAT_VERSION,
            fingerprint: "veraltet".into(),
            sample_rate: 44_100,
            frames: 0,
            duration_secs: 0.0,
            bpm: None,
            beat_anchor_frames: None,
            bpm_confidence: None,
            musical_key: None,
            key_confidence: None,
            peaks: vec![],
        };
        analysis.version = FORMAT_VERSION + 1;
        store.save(&analysis).expect("speichern");

        assert!(store.load("veraltet").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ein Sidecar aus einer älteren Fassung darf nicht als vollständig
    /// durchgehen.
    ///
    /// Ohne die Versionsprüfung würde `serde(default)` das fehlende Feld zu
    /// `None` machen, und `None` heißt hier „geprüft, keine Tonart gefunden" —
    /// eine Falschaussage. Verworfen wird es, und dann rechnet die Analyse neu.
    #[test]
    fn ein_sidecar_ohne_tonart_wird_neu_gerechnet_statt_ergaenzt() {
        let dir = std::env::temp_dir().join("musik-analysis-test-alt");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);

        // So sah ein Sidecar der Version 1 aus — ohne die beiden neuen Felder.
        let alt = r#"{
            "version": 1,
            "fingerprint": "alt",
            "sample_rate": 44100,
            "frames": 1000,
            "duration_secs": 0.02,
            "bpm": 128.0,
            "beat_anchor_frames": 0,
            "bpm_confidence": 0.9,
            "peaks": []
        }"#;
        std::fs::write(store.path_for("alt"), alt).unwrap();

        assert!(store.load("alt").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
