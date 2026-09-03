//! Die Gliederung eines Tracks als Wert.
//!
//! Nur die Begriffe, nicht ihre Erkennung — die steht in `analysis::struktur`
//! und braucht FFT, Hüllkurven und Schwellen. Hier liegt, was jeder lesen
//! muss, der mit einem Track umgeht: das Deck, das Pult, die Oberfläche.
//!
//! Dieselbe Aufteilung wie bei [`crate::Tonart`], und aus demselben Grund:
//! `control` soll wissen, was ein Outro ist, ohne den halben Analysestapel
//! mitzuziehen.

/// Wie lang eine Phrase standardmäßig ist.
///
/// Sechzehn Beats sind vier Takte — die Gruppe, in der House und Techno gebaut
/// sind und an deren Grenzen ein Übergang sitzt. Wer anderes auflegt, schreibt
/// `deckN.phrase_beats`.
///
/// Steht hier und nicht in `control` oder `analysis`, weil beide dieselbe Zahl
/// meinen müssen: Die Gliederung schneidet auf Phrasengrenzen, und `in phrase`
/// zielt darauf. Zwei Konstanten wären zwei Stellen zum Auseinanderlaufen.
pub const PHRASE_BEATS: f64 = 16.0;

/// Was für ein Teil eines Tracks das ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Intro,
    Aufbau,
    Drop,
    Break,
    Outro,
    /// Läuft, ohne sich als eines der anderen zu erkennen zu geben.
    ///
    /// Kein Verlegenheitswert: Die meisten Tracks haben Strecken, die schlicht
    /// laufen. Sie „Drop" zu nennen, weil ein Name dastehen soll, wäre die
    /// schlechtere Antwort.
    Teil,
}

impl Art {
    pub fn name(&self) -> &'static str {
        match self {
            Art::Intro => "intro",
            Art::Aufbau => "aufbau",
            Art::Drop => "drop",
            Art::Break => "break",
            Art::Outro => "outro",
            Art::Teil => "teil",
        }
    }

    pub fn parse(text: &str) -> Option<Art> {
        match text {
            "intro" => Some(Art::Intro),
            "aufbau" => Some(Art::Aufbau),
            "drop" => Some(Art::Drop),
            "break" => Some(Art::Break),
            "outro" => Some(Art::Outro),
            "teil" => Some(Art::Teil),
            _ => None,
        }
    }

    /// Alle Arten, für Katalog und Auswahl.
    pub const ALLE: [Art; 6] = [
        Art::Intro,
        Art::Aufbau,
        Art::Drop,
        Art::Break,
        Art::Outro,
        Art::Teil,
    ];
}

/// Ein Abschnitt des Tracks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Abschnitt {
    /// Anfang in Sample-Frames. **Die maßgebliche Angabe** — der Beat ist
    /// daraus gerechnet und dient dem Lesen.
    pub von_frames: u64,
    /// Ende in Sample-Frames, ausschließlich.
    pub bis_frames: u64,
    pub von_beat: f64,
    pub bis_beat: f64,
    pub art: Art,
    /// Mittlerer Pegel, 0..1 als Anteil am lautesten Abschnitt des Tracks.
    pub pegel: f32,
    /// Mittlerer Bassanteil, ebenso relativ.
    pub bass: f32,
    /// Mittlere Onset-Dichte, ebenso relativ.
    pub dichte: f32,
}

impl Abschnitt {
    pub fn beats(&self) -> f64 {
        self.bis_beat - self.von_beat
    }
}

/// Die Gliederung eines Tracks.
#[derive(Debug, Clone, PartialEq)]
pub struct Struktur {
    pub abschnitte: Vec<Abschnitt>,
    pub phrase_beats: f64,
}

impl Struktur {
    /// Wo der eingehende Track einsetzen sollte.
    ///
    /// Der Anfang des ersten Abschnitts — und der liegt bauartbedingt auf einer
    /// Phrasengrenze. Das ist der ganze Unterschied zu „Sekunde 0": Was davor
    /// liegt, ist Vorlauf und gehört nicht in den Mix.
    pub fn einstieg_frames(&self) -> Option<u64> {
        self.abschnitte.first().map(|a| a.von_frames)
    }

    /// Wo das Outro anfängt — die Stelle, an der ein Übergang liegen darf.
    ///
    /// `None`, wenn der Track keines hat. Das ist eine Auskunft und kein
    /// Mangel: Nicht jede Produktion blendet aus.
    pub fn outro_frames(&self) -> Option<u64> {
        self.abschnitte
            .iter()
            .find(|a| a.art == Art::Outro)
            .map(|a| a.von_frames)
    }

    /// Wie lang das Intro ist, in Beats. `None` ohne Intro.
    ///
    /// Damit wird „ein Track mit langem Intro" eine Anforderung, die ein Agent
    /// stellen kann.
    pub fn intro_beats(&self) -> Option<f64> {
        self.abschnitte
            .iter()
            .find(|a| a.art == Art::Intro)
            .map(|a| a.beats())
    }

    /// In welchem Abschnitt ein Frame liegt.
    pub fn bei_frames(&self, frames: u64) -> Option<&Abschnitt> {
        self.abschnitte
            .iter()
            .find(|a| frames >= a.von_frames && frames < a.bis_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abschnitt(art: Art, von: u64, bis: u64) -> Abschnitt {
        Abschnitt {
            von_frames: von,
            bis_frames: bis,
            von_beat: von as f64 / 100.0,
            bis_beat: bis as f64 / 100.0,
            art,
            pegel: 0.5,
            bass: 0.5,
            dichte: 0.5,
        }
    }

    fn beispiel() -> Struktur {
        Struktur {
            phrase_beats: PHRASE_BEATS,
            abschnitte: vec![
                abschnitt(Art::Intro, 100, 1_000),
                abschnitt(Art::Drop, 1_000, 3_000),
                abschnitt(Art::Outro, 3_000, 4_000),
            ],
        }
    }

    #[test]
    fn jede_art_ueberlebt_ihren_namen() {
        for art in Art::ALLE {
            assert_eq!(Art::parse(art.name()), Some(art));
        }
        assert_eq!(Art::parse("refrain"), None);
    }

    /// Der Einstieg ist der Anfang des ersten Abschnitts, nicht Frame 0.
    #[test]
    fn der_einstieg_ist_der_anfang_des_ersten_abschnitts() {
        assert_eq!(beispiel().einstieg_frames(), Some(100));
        assert_eq!(
            Struktur {
                phrase_beats: PHRASE_BEATS,
                abschnitte: Vec::new()
            }
            .einstieg_frames(),
            None
        );
    }

    #[test]
    fn outro_und_intro_lassen_sich_finden() {
        let s = beispiel();
        assert_eq!(s.outro_frames(), Some(3_000));
        assert_eq!(s.intro_beats(), Some(9.0));
    }

    /// Ohne Outro wird keines erfunden. Nicht jede Produktion blendet aus.
    #[test]
    fn ohne_outro_kommt_nichts_zurueck() {
        let s = Struktur {
            phrase_beats: PHRASE_BEATS,
            abschnitte: vec![abschnitt(Art::Drop, 0, 1_000)],
        };
        assert_eq!(s.outro_frames(), None);
        assert_eq!(s.intro_beats(), None);
    }

    #[test]
    fn ein_abschnitt_laesst_sich_ueber_den_frame_finden() {
        let s = beispiel();
        assert_eq!(s.bei_frames(100).map(|a| a.art), Some(Art::Intro));
        assert_eq!(s.bei_frames(999).map(|a| a.art), Some(Art::Intro));
        assert_eq!(s.bei_frames(1_000).map(|a| a.art), Some(Art::Drop));
        // Vor dem ersten Abschnitt liegt der Vorlauf, dahinter nichts mehr.
        assert!(s.bei_frames(0).is_none());
        assert!(s.bei_frames(4_000).is_none());
    }
}
