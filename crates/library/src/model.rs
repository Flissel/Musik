//! Datenmodell der Library.

use audio_core::{Beatgrid, Tonart};

/// Woher ein Track stammt. Bestimmt mit, welche Rechtepflichten daran hängen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Eigene Datei auf der Platte.
    File,
    /// Aus einer Sample-Datenbank geladen — Lizenz und Urheber sind Pflichtfelder.
    Sample,
    /// Von einem Generierungsdienst erzeugt.
    Generated,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::File => "file",
            Source::Sample => "sample",
            Source::Generated => "generated",
        }
    }

    pub fn parse(raw: &str) -> Source {
        match raw {
            "sample" => Source::Sample,
            "generated" => Source::Generated,
            _ => Source::File,
        }
    }
}

/// Ein Track in der Sammlung.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRecord {
    pub id: Option<i64>,
    /// Inhalts-Hash aus der Analyse. `None`, solange nicht analysiert.
    pub fingerprint: Option<String>,
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub duration_secs: Option<f64>,
    pub bpm: Option<f32>,
    /// Lage des ersten Beats in Millisekunden.
    ///
    /// Millisekunden statt Frames, weil die Library die Tauschschicht ist:
    /// Traktor speichert so, ein resampelter Track hätte andere Frame-Zahlen,
    /// und ein Wert ohne zugehörige Samplerate wäre mehrdeutig.
    pub beat_anchor_ms: Option<f64>,
    /// Tonart als Text, wie sie in der Quelle stand.
    pub musical_key: Option<String>,

    /// Lizenz und Urheber.
    ///
    /// **Pflichtfelder für alles aus Sample-Datenbanken.** CC BY verlangt
    /// Namensnennung auch bei nicht-kommerzieller Nutzung, und nachträglich
    /// lässt sich die Herkunft nicht mehr rekonstruieren, wenn erst tausend
    /// Dateien ohne sie im Ordner liegen. Deshalb stehen sie hier von der
    /// ersten Migration an und nicht in einer späteren.
    pub license: Option<String>,
    pub attribution: Option<String>,

    pub source: Source,
}

impl TrackRecord {
    /// Neuer Eintrag aus einem Dateipfad, ohne Metadaten.
    pub fn from_path(path: impl Into<String>) -> Self {
        TrackRecord {
            id: None,
            fingerprint: None,
            path: path.into(),
            title: None,
            artist: None,
            album: None,
            genre: None,
            duration_secs: None,
            bpm: None,
            beat_anchor_ms: None,
            musical_key: None,
            license: None,
            attribution: None,
            source: Source::File,
        }
    }

    pub fn beatgrid(&self, sample_rate: u32) -> Option<Beatgrid> {
        let bpm = self.bpm?;
        let anchor = ms_to_frames(self.beat_anchor_ms.unwrap_or(0.0), sample_rate);
        let grid = Beatgrid::new(bpm, anchor, 1.0);
        grid.is_usable().then_some(grid)
    }

    pub fn set_beatgrid(&mut self, grid: Option<Beatgrid>, sample_rate: u32) {
        match grid {
            Some(g) => {
                self.bpm = Some(g.bpm);
                self.beat_anchor_ms = Some(frames_to_ms(g.anchor_frames, sample_rate));
            }
            None => {
                self.bpm = None;
                self.beat_anchor_ms = None;
            }
        }
    }

    /// Ob die Rechtelage vollständig ist.
    ///
    /// Für eigene Dateien belanglos, für Samples entscheidend — und deshalb
    /// abfragbar, statt nur dokumentiert zu sein.
    pub fn attribution_complete(&self) -> bool {
        match self.source {
            Source::Sample => {
                self.license.as_ref().is_some_and(|s| !s.trim().is_empty())
                    && self
                        .attribution
                        .as_ref()
                        .is_some_and(|s| !s.trim().is_empty())
            }
            _ => true,
        }
    }
}

/// Ein Hot Cue oder Grid-Marker.
#[derive(Debug, Clone, PartialEq)]
pub struct CueRecord {
    pub id: Option<i64>,
    pub track_id: i64,
    /// Hot-Cue-Nummer, `None` bei Markern ohne Taste (etwa dem Grid-Anker).
    pub hotcue: Option<u8>,
    /// Position in Millisekunden — siehe [`TrackRecord::beat_anchor_ms`].
    pub position_ms: f64,
    pub name: Option<String>,
    pub kind: CueKind,
}

impl CueRecord {
    pub fn frame(&self, sample_rate: u32) -> u64 {
        ms_to_frames(self.position_ms, sample_rate)
    }
}

/// Millisekunden in Sample-Frames. Negatives wird auf null gezogen.
pub fn ms_to_frames(ms: f64, sample_rate: u32) -> u64 {
    (ms.max(0.0) / 1_000.0 * sample_rate as f64).round() as u64
}

pub fn frames_to_ms(frames: u64, sample_rate: u32) -> f64 {
    frames as f64 * 1_000.0 / sample_rate as f64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueKind {
    Cue,
    Grid,
    Loop,
    FadeIn,
    FadeOut,
    Load,
}

impl CueKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CueKind::Cue => "cue",
            CueKind::Grid => "grid",
            CueKind::Loop => "loop",
            CueKind::FadeIn => "fade_in",
            CueKind::FadeOut => "fade_out",
            CueKind::Load => "load",
        }
    }

    pub fn parse(raw: &str) -> CueKind {
        match raw {
            "grid" => CueKind::Grid,
            "loop" => CueKind::Loop,
            "fade_in" => CueKind::FadeIn,
            "fade_out" => CueKind::FadeOut,
            "load" => CueKind::Load,
            _ => CueKind::Cue,
        }
    }
}

/// Suchanfrage an die Library.
#[derive(Debug, Clone, Default)]
pub struct Query {
    /// Freitext über Titel, Künstler und Album.
    pub text: Option<String>,
    pub bpm_min: Option<f32>,
    pub bpm_max: Option<f32>,
    pub genre: Option<String>,
    /// Zulässige Tonarten, in allen Schreibweisen, die gemeint sein können.
    ///
    /// Eine Liste statt eines einzelnen Werts, weil harmonisch nie genau eine
    /// Tonart passt — siehe [`Query::harmonic_with`].
    pub keys: Option<Vec<String>>,
    pub limit: Option<u32>,
}

impl Query {
    pub fn text(query: impl Into<String>) -> Self {
        Query {
            text: Some(query.into()),
            ..Default::default()
        }
    }

    /// Tracks, die tempomäßig zu `bpm` passen — Standard sind ±6 %, der
    /// Bereich, den ein Pitchfader ohne hörbare Verfärbung hergibt.
    pub fn mixable_with(bpm: f32, toleranz: f32) -> Self {
        Query {
            bpm_min: Some(bpm * (1.0 - toleranz)),
            bpm_max: Some(bpm * (1.0 + toleranz)),
            ..Default::default()
        }
    }

    /// Tracks, deren Tonart harmonisch zu `tonart` passt.
    ///
    /// Gesucht wird über beide Schreibweisen — `Am` und `8A` —, weil in der
    /// Sammlung steht, was die Quelle geschrieben hat: Die eigene Analyse legt
    /// Namen ab, Traktor schreibt mal das eine, mal das andere. Eine dritte
    /// Schreibweise (`A min` etwa) wird nicht gefunden; das ist eine bewusste
    /// Grenze und keine stille.
    pub fn harmonic_with(tonart: Tonart) -> Self {
        let keys = tonart
            .verwandte()
            .iter()
            .flat_map(|t| [t.name(), t.camelot()])
            .collect();

        Query {
            keys: Some(keys),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    #[test]
    fn beatgrid_geht_hin_und_zurueck() {
        let mut track = TrackRecord::from_path("/a.mp3");
        assert!(track.beatgrid(RATE).is_none());

        track.set_beatgrid(Some(Beatgrid::new(128.0, 4_410, 1.0)), RATE);
        assert_eq!(track.beat_anchor_ms, Some(100.0));

        let grid = track.beatgrid(RATE).expect("kein Grid");
        assert!((grid.bpm - 128.0).abs() < 1e-6);
        assert_eq!(grid.anchor_frames, 4_410);

        track.set_beatgrid(None, RATE);
        assert!(track.beatgrid(RATE).is_none());
    }

    #[test]
    fn millisekunden_ueberstehen_einen_ratenwechsel() {
        // Derselbe Zeitpunkt, zwei Samplerates — genau dafür ist ms da.
        let mut track = TrackRecord::from_path("/a.mp3");
        track.set_beatgrid(Some(Beatgrid::new(120.0, 4_410, 1.0)), 44_100);

        let bei_48k = track.beatgrid(48_000).unwrap();
        assert_eq!(bei_48k.anchor_frames, 4_800);
    }

    #[test]
    fn negative_positionen_werden_abgefangen() {
        assert_eq!(ms_to_frames(-5.0, 48_000), 0);
        assert_eq!(ms_to_frames(0.0, 48_000), 0);
        assert_eq!(ms_to_frames(1_000.0, 48_000), 48_000);
    }

    #[test]
    fn samples_brauchen_lizenz_und_urheber() {
        let mut track = TrackRecord::from_path("/kick.wav");
        track.source = Source::Sample;
        assert!(
            !track.attribution_complete(),
            "Sample ohne Rechte durchgelassen"
        );

        track.license = Some("CC BY 4.0".into());
        assert!(!track.attribution_complete(), "Urheber fehlt noch");

        track.attribution = Some("jemand auf Freesound".into());
        assert!(track.attribution_complete());
    }

    #[test]
    fn leerzeichen_zaehlen_nicht_als_angabe() {
        let mut track = TrackRecord::from_path("/kick.wav");
        track.source = Source::Sample;
        track.license = Some("   ".into());
        track.attribution = Some("".into());

        assert!(!track.attribution_complete());
    }

    #[test]
    fn eigene_dateien_brauchen_keine_angaben() {
        let track = TrackRecord::from_path("/eigener-track.mp3");
        assert_eq!(track.source, Source::File);
        assert!(track.attribution_complete());
    }

    #[test]
    fn mixbarer_bereich_umfasst_das_tempo() {
        let q = Query::mixable_with(128.0, 0.06);
        assert!((q.bpm_min.unwrap() - 120.32).abs() < 0.01);
        assert!((q.bpm_max.unwrap() - 135.68).abs() < 0.01);
    }

    #[test]
    fn quellen_und_marker_ueberleben_die_textform() {
        for s in [Source::File, Source::Sample, Source::Generated] {
            assert_eq!(Source::parse(s.as_str()), s);
        }
        for k in [
            CueKind::Cue,
            CueKind::Grid,
            CueKind::Loop,
            CueKind::FadeIn,
            CueKind::FadeOut,
            CueKind::Load,
        ] {
            assert_eq!(CueKind::parse(k.as_str()), k);
        }
    }
}
