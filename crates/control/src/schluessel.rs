//! Adressierung eines Controls.
//!
//! Zweiteilig wie bei Mixxx — Gruppe und Element — aber in einer Schreibweise,
//! die man tippen und in eine URL schreiben kann: `deck1.play` statt
//! `[Channel1],play`.
//!
//! Die Gruppe ist getippt und nicht bloß eine Zeichenkette. `deck1` und
//! `channel1` sind nicht dasselbe: Das Deck ist der Abspieler, der Kanal ist
//! der Zug am Mischpult. Bei Mixxx fallen beide in `[Channel1]` zusammen, was
//! genau so lange gutgeht, bis ein Kanal etwas anderes als ein Deck führt —
//! bei uns der AUX-Eingang.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gruppe {
    /// Ein Abspieler. Eins-basiert nach außen, null-basiert im Inneren.
    Deck(usize),
    /// Ein Zug am Mischpult. Führt ein Deck oder den AUX-Eingang.
    Kanal(usize),
    /// Summe, Crossfader, Kopfhörer.
    Master,
}

impl fmt::Display for Gruppe {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Gruppe::Deck(i) => write!(f, "deck{}", i + 1),
            Gruppe::Kanal(i) => write!(f, "channel{}", i + 1),
            Gruppe::Master => f.write_str("master"),
        }
    }
}

impl Gruppe {
    pub fn parse(text: &str) -> Option<Gruppe> {
        if text == "master" {
            return Some(Gruppe::Master);
        }

        for (prefix, bauen) in [
            ("deck", Gruppe::Deck as fn(usize) -> Gruppe),
            ("channel", Gruppe::Kanal as fn(usize) -> Gruppe),
        ] {
            if let Some(rest) = text.strip_prefix(prefix) {
                // Eins-basiert nach außen: „deck0" gibt es nicht, und es
                // stillschweigend als Deck 1 zu lesen würde Tippfehler
                // verstecken.
                let nummer: usize = rest.parse().ok()?;
                return nummer.checked_sub(1).map(bauen);
            }
        }

        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Schluessel {
    pub gruppe: Gruppe,
    pub element: String,
}

impl Schluessel {
    pub fn neu(gruppe: Gruppe, element: impl Into<String>) -> Schluessel {
        Schluessel {
            gruppe,
            element: element.into(),
        }
    }

    pub fn parse(text: &str) -> Option<Schluessel> {
        let (gruppe, element) = text.split_once('.')?;
        if element.is_empty() || element.contains('.') {
            return None;
        }
        Some(Schluessel::neu(Gruppe::parse(gruppe)?, element))
    }
}

impl fmt::Display for Schluessel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.gruppe, self.element)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schluessel_ueberleben_hin_und_zurueck() {
        for text in [
            "deck1.play",
            "deck2.tempo",
            "channel3.eq_low",
            "master.gain",
        ] {
            let k = Schluessel::parse(text).unwrap_or_else(|| panic!("{text} nicht lesbar"));
            assert_eq!(k.to_string(), text);
        }
    }

    #[test]
    fn nummern_sind_nach_aussen_eins_basiert() {
        assert_eq!(Gruppe::parse("deck1"), Some(Gruppe::Deck(0)));
        assert_eq!(Gruppe::parse("channel4"), Some(Gruppe::Kanal(3)));

        // Kein stilles Umdeuten: Deck 0 gibt es nicht.
        assert_eq!(Gruppe::parse("deck0"), None);
    }

    #[test]
    fn deck_und_kanal_sind_nicht_dasselbe() {
        // Der AUX-Eingang ist ein Kanal ohne Deck. Fielen beide zusammen,
        // gäbe es für ihn keine Adresse.
        assert_ne!(Gruppe::parse("deck1"), Gruppe::parse("channel1"));
    }

    #[test]
    fn kaputte_schluessel_werden_abgewiesen() {
        for text in [
            "",
            "deck1",
            "deck1.",
            ".play",
            "deck1.play.now",
            "deckX.play",
            "quatsch.play",
        ] {
            assert_eq!(
                Schluessel::parse(text),
                None,
                "{text} hätte scheitern müssen"
            );
        }
    }
}
