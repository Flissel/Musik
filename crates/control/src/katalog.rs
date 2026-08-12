//! Was es gibt — und was es bedeutet.
//!
//! Das ist der Teil, den Mixxx nicht hat. Dort ist die Liste der Controls
//! *Dokumentation*: Sie steht im Handbuch, und wer von außen steuern will,
//! liest sie und tippt die Namen ab. Läuft das Programm, lässt sich nicht
//! erfragen, was es kann.
//!
//! Hier trägt jedes Control seinen Bereich, seine Einheit, seinen Standardwert
//! und einen Satz zur Bedeutung mit sich. Ein Agent, ein Controller-Mapping
//! oder ein Skript kann das Pult zur Laufzeit **fragen**, statt es zu wissen.
//! Genau darauf setzt später die MCP-Schicht auf: Werkzeugbeschreibungen lassen
//! sich daraus erzeugen, statt sie von Hand doppelt zu pflegen.
//!
//! Die Namen sind englisch, obwohl der Rest des Codes deutsch kommentiert ist.
//! Sie sind die Schnittstelle nach außen, und die spricht die Sprache, die
//! Controller-Mappings und Agenten ohnehin verwenden.

use crate::wert::{Art, Einheit};

#[derive(Debug, Clone)]
pub struct Beschreibung {
    pub element: &'static str,
    pub art: Art,
    /// Nur bei [`Art::Zahl`]: kleinster und größter sinnvoller Wert.
    pub bereich: Option<(f64, f64)>,
    pub einheit: Einheit,
    pub auswahl: &'static [&'static str],
    pub schreibbar: bool,
    pub text: &'static str,
}

impl Beschreibung {
    /// Wert innerhalb des Bereichs zurechtstutzen.
    ///
    /// Ein Fader über 1.0 ist kein Fehler, den man dem Aufrufer um die Ohren
    /// hauen muss — ein MIDI-Regler, der 127 sendet, meint das Maximum. Werte
    /// außerhalb werden deshalb begrenzt, nicht abgelehnt.
    pub fn begrenzen(&self, wert: f64) -> f64 {
        match self.bereich {
            Some((min, max)) => wert.clamp(min, max),
            None => wert,
        }
    }

    /// 0..1 in den echten Bereich. Für Controller, die nur Prozent kennen.
    pub fn aus_normiert(&self, norm: f64) -> f64 {
        match self.bereich {
            Some((min, max)) => min + norm.clamp(0.0, 1.0) * (max - min),
            None => norm,
        }
    }

    /// Echter Bereich nach 0..1.
    pub fn nach_normiert(&self, wert: f64) -> f64 {
        match self.bereich {
            Some((min, max)) if max > min => ((wert - min) / (max - min)).clamp(0.0, 1.0),
            _ => wert,
        }
    }
}

const fn zahl(
    element: &'static str,
    min: f64,
    max: f64,
    einheit: Einheit,
    schreibbar: bool,
    text: &'static str,
) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Zahl,
        bereich: Some((min, max)),
        einheit,
        auswahl: &[],
        schreibbar,
        text,
    }
}

const fn schalter(element: &'static str, schreibbar: bool, text: &'static str) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Schalter,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &[],
        schreibbar,
        text,
    }
}

const fn text_feld(element: &'static str, text: &'static str) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Text,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &[],
        schreibbar: false,
        text,
    }
}

/// Obergrenze des Tempo-Reglers. ±8 % ist der Regelweg eines Technics-Decks
/// und damit das, was DJs im Griff haben.
pub const TEMPO_MIN: f64 = 0.92;
pub const TEMPO_MAX: f64 = 1.08;

/// Hot Cues, die es je Deck gibt. Muss zu `audio_core::deck::HOT_CUES` passen.
pub const HOT_CUES: usize = audio_core::deck::HOT_CUES;

pub static DECK: &[Beschreibung] = &[
    schalter("play", true, "Läuft das Deck"),
    zahl(
        "position",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        true,
        "Abspielposition; Schreiben springt",
    ),
    zahl(
        "duration",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Länge des geladenen Tracks",
    ),
    zahl(
        "bpm",
        0.0,
        400.0,
        Einheit::Bpm,
        false,
        "Tempo einschließlich Tempo-Regler",
    ),
    zahl(
        "bpm_grid",
        0.0,
        400.0,
        Einheit::Bpm,
        false,
        "Tempo des Beatgrids, ohne Regler",
    ),
    zahl(
        "tempo",
        TEMPO_MIN,
        TEMPO_MAX,
        Einheit::Faktor,
        true,
        "Tempo-Regler; 1.0 ist die Originalgeschwindigkeit",
    ),
    schalter("keylock", true, "Tonhöhe beim Tempowechsel halten"),
    zahl(
        "beat_phase",
        0.0,
        1.0,
        Einheit::Beats,
        false,
        "Lage im Beat, 0 ist auf dem Schlag",
    ),
    schalter("loop_active", true, "Läuft gerade eine Schleife"),
    zahl(
        "loop_beats",
        0.0,
        64.0,
        Einheit::Beats,
        true,
        "Schleife dieser Länge ab der Position setzen",
    ),
    text_feld("title", "Titel des geladenen Tracks"),
    text_feld("artist", "Künstler des geladenen Tracks"),
];

/// Hot Cues heißen `cue1` bis `cue8` und lassen sich nicht als feste Liste
/// hinschreiben, ohne sie achtmal zu wiederholen.
pub fn hot_cue_beschreibung(index: usize) -> Option<Beschreibung> {
    if index >= HOT_CUES {
        return None;
    }

    Some(Beschreibung {
        // Der Name wird zur Laufzeit gebraucht, `element` ist aber `'static`.
        // Deshalb eine kleine feste Tabelle statt einer geleakten Zeichenkette.
        element: HOT_CUE_NAMEN[index],
        art: Art::Zahl,
        bereich: Some((0.0, f64::MAX)),
        einheit: Einheit::Sekunden,
        auswahl: &[],
        schreibbar: true,
        text: "Hot Cue; Schreiben setzt ihn, Lesen gibt seine Position oder '-'",
    })
}

static HOT_CUE_NAMEN: [&str; HOT_CUES] = [
    "cue1", "cue2", "cue3", "cue4", "cue5", "cue6", "cue7", "cue8",
];

pub static KANAL: &[Beschreibung] = &[
    zahl(
        "trim",
        0.0,
        2.0,
        Einheit::Faktor,
        true,
        "Eingangsverstärkung vor dem EQ",
    ),
    zahl(
        "eq_low",
        0.0,
        2.0,
        Einheit::Faktor,
        true,
        "Bässe; 0 ist ein echter Kill",
    ),
    zahl("eq_mid", 0.0, 2.0, Einheit::Faktor, true, "Mitten"),
    zahl("eq_high", 0.0, 2.0, Einheit::Faktor, true, "Höhen"),
    zahl(
        "filter",
        -1.0,
        1.0,
        Einheit::Bipolar,
        true,
        "DJ-Filter; negativ Tiefpass, positiv Hochpass",
    ),
    zahl("fader", 0.0, 1.0, Einheit::Faktor, true, "Linefader"),
    schalter("cue", true, "Kanal auf den Kopfhörer legen"),
    Beschreibung {
        element: "assign",
        art: Art::Auswahl,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &["a", "b", "thru"],
        schreibbar: true,
        text: "Seite am Crossfader; 'thru' geht am Crossfader vorbei",
    },
];

pub static MASTER: &[Beschreibung] = &[
    zahl(
        "crossfader",
        -1.0,
        1.0,
        Einheit::Bipolar,
        true,
        "Crossfader; -1 ist ganz A, +1 ganz B",
    ),
    zahl(
        "crossfader_curve",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Kurve; 0 weich, 1 hart",
    ),
    zahl("gain", 0.0, 1.5, Einheit::Faktor, true, "Summenlautstärke"),
    zahl(
        "cue_gain",
        0.0,
        1.5,
        Einheit::Faktor,
        true,
        "Kopfhörerlautstärke",
    ),
    zahl(
        "cue_mix",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Kopfhörer zwischen Vorhören (0) und Summe (1)",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jedes_control_beschreibt_sich_selbst() {
        // Der ganze Sinn des Katalogs: Wer fragt, bekommt eine Antwort, die
        // ohne Handbuch trägt.
        let alle = DECK.iter().chain(KANAL).chain(MASTER);
        for b in alle {
            assert!(!b.element.is_empty());
            assert!(!b.text.is_empty(), "{} hat keine Beschreibung", b.element);
            if b.art == Art::Zahl {
                assert!(
                    b.bereich.is_some(),
                    "{} ist eine Zahl ohne Bereich",
                    b.element
                );
            }
            if b.art == Art::Auswahl {
                assert!(
                    !b.auswahl.is_empty(),
                    "{} ist eine Auswahl ohne Optionen",
                    b.element
                );
            }
        }
    }

    #[test]
    fn namen_kommen_innerhalb_einer_gruppe_nur_einmal_vor() {
        for gruppe in [DECK, KANAL, MASTER] {
            let mut namen: Vec<_> = gruppe.iter().map(|b| b.element).collect();
            let vorher = namen.len();
            namen.sort_unstable();
            namen.dedup();
            assert_eq!(namen.len(), vorher, "doppelter Name in einer Gruppe");
        }
    }

    #[test]
    fn normierung_geht_hin_und_zurueck() {
        let b = &KANAL[0]; // trim, 0..2
        assert_eq!(b.aus_normiert(0.0), 0.0);
        assert_eq!(b.aus_normiert(1.0), 2.0);
        assert_eq!(b.nach_normiert(1.0), 0.5);

        // Ein bipolares Control hat seine Mitte bei 0.5 normiert — das ist der
        // Punkt, an dem ein MIDI-Regler in der Raste steht.
        let filter = KANAL.iter().find(|b| b.element == "filter").unwrap();
        assert_eq!(filter.nach_normiert(0.0), 0.5);
        assert_eq!(filter.aus_normiert(0.5), 0.0);
    }

    #[test]
    fn werte_ausserhalb_werden_begrenzt_statt_abgelehnt() {
        // Ein MIDI-Regler auf Anschlag meint das Maximum, keinen Fehler.
        let fader = KANAL.iter().find(|b| b.element == "fader").unwrap();
        assert_eq!(fader.begrenzen(1.7), 1.0);
        assert_eq!(fader.begrenzen(-0.2), 0.0);
    }

    #[test]
    fn es_gibt_genau_so_viele_hot_cues_wie_im_deck() {
        assert!(hot_cue_beschreibung(HOT_CUES - 1).is_some());
        assert!(hot_cue_beschreibung(HOT_CUES).is_none());
        assert_eq!(hot_cue_beschreibung(0).unwrap().element, "cue1");
    }

    #[test]
    fn nur_gelesene_controls_sind_auch_als_solche_markiert() {
        let dauer = DECK.iter().find(|b| b.element == "duration").unwrap();
        assert!(
            !dauer.schreibbar,
            "die Länge eines Tracks lässt sich nicht setzen"
        );
    }
}
