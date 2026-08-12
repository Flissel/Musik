//! Werte, die über das Steuerpult laufen.
//!
//! Bewusst **typisiert** statt alles als Fließkommazahl. Mixxx macht jedes
//! Control zu einem `double` — `play` ist dort 0.0 oder 1.0, die
//! Crossfader-Zuweisung eine durchnummerierte Zahl. Das ist einfach zu bauen
//! und schwer zu benutzen: Wer von außen steuert, muss wissen, dass 2.0 bei
//! `orientation` „rechts" heißt, und ein Tippfehler wird zu einem gültigen
//! Wert statt zu einem Fehler.
//!
//! Hier ist ein Schalter ein Schalter, eine Auswahl trägt ihre erlaubten Namen
//! und eine Zahl ihren Bereich. Falsches lässt sich damit zurückweisen, statt
//! es stumm zu übernehmen.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Wert {
    Schalter(bool),
    Zahl(f64),
    Auswahl(String),
    Text(String),
    /// Ein Wert, den es gerade nicht gibt — ein ungesetzter Hot Cue, das Tempo
    /// eines Decks ohne Beatgrid.
    Leer,
}

impl Wert {
    pub fn als_zahl(&self) -> Option<f64> {
        match self {
            Wert::Zahl(v) => Some(*v),
            Wert::Schalter(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn als_schalter(&self) -> Option<bool> {
        match self {
            Wert::Schalter(b) => Some(*b),
            Wert::Zahl(v) => Some(*v >= 0.5),
            _ => None,
        }
    }

    pub fn als_text(&self) -> Option<&str> {
        match self {
            Wert::Text(s) | Wert::Auswahl(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for Wert {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Wert::Schalter(true) => f.write_str("1"),
            Wert::Schalter(false) => f.write_str("0"),
            // Kurz halten, aber nichts verschlucken: Positionen brauchen
            // Millisekunden-Auflösung, Verstärkungen nicht.
            Wert::Zahl(v) if v.fract() == 0.0 && v.abs() < 1e15 => write!(f, "{v:.0}"),
            Wert::Zahl(v) => write!(f, "{v:.6}"),
            Wert::Auswahl(s) | Wert::Text(s) => f.write_str(s),
            Wert::Leer => f.write_str("-"),
        }
    }
}

/// Was für ein Wert an einem Control hängt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    Schalter,
    Zahl,
    Auswahl,
    Text,
}

impl Art {
    pub fn name(&self) -> &'static str {
        match self {
            Art::Schalter => "schalter",
            Art::Zahl => "zahl",
            Art::Auswahl => "auswahl",
            Art::Text => "text",
        }
    }
}

/// Wofür die Zahl steht. Ohne das ist „0.8" nicht interpretierbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Einheit {
    Keine,
    /// 1.0 = unverändert. Tempo, Verstärkung, EQ.
    Faktor,
    Sekunden,
    Beats,
    Bpm,
    /// −1 bis +1, Mitte 0. Crossfader, Filter.
    Bipolar,
}

impl Einheit {
    pub fn name(&self) -> &'static str {
        match self {
            Einheit::Keine => "-",
            Einheit::Faktor => "faktor",
            Einheit::Sekunden => "s",
            Einheit::Beats => "beats",
            Einheit::Bpm => "bpm",
            Einheit::Bipolar => "bipolar",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schalter_werden_als_null_und_eins_geschrieben() {
        assert_eq!(Wert::Schalter(true).to_string(), "1");
        assert_eq!(Wert::Schalter(false).to_string(), "0");
    }

    #[test]
    fn zahlen_verlieren_beim_schreiben_keine_aufloesung() {
        // Eine Position auf die zweite Nachkommastelle zu runden wäre bei
        // 44 100 Hz ein Fehler von hunderten Samples.
        assert_eq!(Wert::Zahl(12.345678).to_string(), "12.345678");
        assert_eq!(Wert::Zahl(4.0).to_string(), "4");
    }

    #[test]
    fn ein_leerer_wert_ist_kein_null() {
        // Ein ungesetzter Hot Cue liegt nicht auf Sekunde 0.
        assert_eq!(Wert::Leer.to_string(), "-");
        assert_eq!(Wert::Leer.als_zahl(), None);
    }
}
