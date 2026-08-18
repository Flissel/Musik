//! Zurückhaltung: ob immer dasselbe gefahren wird.
//!
//! Das Repertoire aus vier Griffen macht Abwechslung **möglich**. Es erzwingt
//! sie nicht — und ein System, das viermal hintereinander dieselbe Blende
//! wählt, klingt weiterhin nach Automat, auch wenn jede einzelne sauber ist.
//! Genau das war der erste selbstgefahrene Übergang, und der Vorwurf daran
//! („ein bisschen herzlos") ist nie mit einer Zahl beantwortet worden.
//!
//! Hier steht sie: wie viele der letzten Übergänge gleich waren.
//!
//! # Was als Übergang zählt
//!
//! **Ein Übergang ist vorbei, wenn der Crossfader die Seite wechselt.** Das ist
//! die einzige Stelle, die sich ohne Raterei bestimmen lässt, und sie zählt
//! genau einmal: Ein Bass-Swap fährt zweimal am Crossfader (erst in die Mitte,
//! dann hinüber), eine Blende in vielen kleinen Schritten, ein Schnitt in einem
//! Sprung — angekommen wird bei allen dreien einmal.
//!
//! Nicht der Wert entscheidet, sondern der Wechsel: Wer `set master.crossfader
//! 1` dreimal schickt, hat einmal übergeblendet, nicht dreimal.
//!
//! **Und es müssen zwei Decks laufen.** Wer vor dem Set den Fader auf die Seite
//! des einzigen laufenden Decks stellt, richtet ein — da ist nichts, wovon oder
//! wohin überzublenden wäre. Das stand hier zunächst falsch und fiel erst am
//! laufenden Programm auf: Nach vier Zeilen Einrichten meldete die Anlage
//! bereits einen Übergang. Alle vier Griffe starten das eingehende Deck, bevor
//! der Fader sich bewegt; zwei laufende Decks sind also kein Sonderfall,
//! sondern das Kennzeichen.
//!
//! Damit ist auch gezählt, was **von Hand** zusammengesetzt wurde und nicht aus
//! dem Repertoire kam. Das ist wichtig: Der erste automatisch gefahrene
//! Übergang bestand aus sieben `when`-Zeilen und keinem einzigen `uebergang`.
//!
//! **Zwei Grenzen sind echt und stehen hier:** Wer nur mit den Kanalfadern
//! mischt und den Crossfader stehen lässt, taucht nicht auf. Und wenn der
//! ausgehende Track ausläuft, bevor die Blende drüben ankommt, fehlt sie —
//! dann lief zuletzt nur noch ein Deck. Beides kommt vor; dann sagt diese Zahl
//! nichts, statt etwas Falsches zu sagen.

use std::collections::VecDeque;

/// Wie viele Übergänge aufgehoben werden.
///
/// Acht sind eine gute halbe Stunde Set — lang genug, um ein Muster zu sehen,
/// kurz genug, dass ein Bediener die Liste noch überblickt.
pub const PLATZ: usize = 8;

/// Ab welcher Crossfader-Stellung er „drüben" ist.
///
/// Nicht exakt 1: Wer über MCP `0.98` schickt, meint dasselbe, und eine
/// Zählung, die an der zweiten Nachkommastelle scheitert, wäre eine Falle.
pub const ANGEKOMMEN: f64 = 0.9;

/// Auf welcher Seite der Crossfader steht: -1 links, 1 rechts, 0 dazwischen.
///
/// Der Wechsel dieser Zahl ist der Übergang — nicht der Wert selbst. Ein Sprung
/// von ganz links nach ganz rechts ist einer, obwohl der Betrag sich nie
/// geändert hat; dreimal derselbe Befehl ist keiner.
pub fn seite(v: f64) -> i8 {
    if v >= ANGEKOMMEN {
        1
    } else if v <= -ANGEKOMMEN {
        -1
    } else {
        0
    }
}

/// Was zuletzt gefahren wurde.
#[derive(Debug, Default, Clone)]
pub struct Vielfalt {
    letzte: VecDeque<String>,
    /// Der Name eines angeforderten Griffs, bis der Crossfader ankommt.
    ///
    /// Ohne den stünde von einem Bass-Swap nur die Form seiner letzten Rampe in
    /// der Erinnerung — dieselbe wie bei einer Blende. Zwei verschiedene Griffe
    /// sähen dann aus wie eine Wiederholung.
    vorgemerkt: Option<String>,
}

impl Vielfalt {
    pub fn neu() -> Vielfalt {
        Vielfalt::default()
    }

    /// Trägt einen Übergang ein.
    ///
    /// `art` ist seine Kennung: der Name des Griffs, wenn er aus dem Repertoire
    /// kam, sonst Form und Länge der Bewegung.
    pub fn merken(&mut self, art: &str) {
        let art = art.trim();
        if art.is_empty() {
            return;
        }
        self.letzte.push_back(art.to_string());
        while self.letzte.len() > PLATZ {
            self.letzte.pop_front();
        }
    }

    /// Merkt vor, welcher Griff gerade angefordert wurde.
    ///
    /// Eingetragen wird er erst, wenn der Crossfader ankommt — ein Griff, den
    /// jemand plant und dann abbricht, hat nicht stattgefunden.
    pub fn vormerken(&mut self, griff: &str) {
        self.vorgemerkt = Some(griff.trim().to_string());
    }

    /// Der Crossfader ist auf einer Seite angekommen.
    ///
    /// `wie` beschreibt die Bewegung, die ihn dorthin gebracht hat — Form und
    /// Länge einer Rampe, oder `schnitt` für einen Sprung. Ein vorgemerkter
    /// Griff hat Vorrang, weil sein Name mehr sagt.
    pub fn angekommen(&mut self, wie: &str) {
        match self.vorgemerkt.take() {
            Some(griff) if !griff.is_empty() => self.merken(&griff),
            _ => self.merken(wie),
        }
    }

    /// Wie viele der letzten Übergänge hintereinander gleich waren.
    ///
    /// 0 heißt: noch keiner. 1 heißt: der letzte war anders als der davor —
    /// also alles in Ordnung. Ab 3 wiederholt sich jemand hörbar.
    pub fn wiederholung(&self) -> usize {
        let Some(letzter) = self.letzte.back() else {
            return 0;
        };
        self.letzte
            .iter()
            .rev()
            .take_while(|a| *a == letzter)
            .count()
    }

    /// Wie viele verschiedene Arten in der Erinnerung stehen.
    pub fn arten(&self) -> usize {
        let mut gesehen: Vec<&String> = Vec::new();
        for a in &self.letzte {
            if !gesehen.contains(&a) {
                gesehen.push(a);
            }
        }
        gesehen.len()
    }

    pub fn ist_leer(&self) -> bool {
        self.letzte.is_empty()
    }

    /// Die letzten Übergänge, ältester zuerst.
    pub fn text(&self) -> String {
        self.letzte.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mit(arten: &[&str]) -> Vielfalt {
        let mut v = Vielfalt::neu();
        for a in arten {
            v.merken(a);
        }
        v
    }

    /// Die Zahl, um die es geht.
    #[test]
    fn wiederholung_zaehlt_die_gleichen_am_ende() {
        assert_eq!(mit(&[]).wiederholung(), 0);
        assert_eq!(mit(&["blende"]).wiederholung(), 1);
        assert_eq!(mit(&["blende", "blende", "blende"]).wiederholung(), 3);
        // Etwas anderes dazwischen bricht die Serie.
        assert_eq!(mit(&["blende", "blende", "schnitt"]).wiederholung(), 1);
        // Und was davor war, zählt nicht mehr mit.
        assert_eq!(
            mit(&["blende", "blende", "schnitt", "schnitt"]).wiederholung(),
            2
        );
    }

    #[test]
    fn die_arten_zaehlen_das_verschiedene() {
        assert_eq!(mit(&[]).arten(), 0);
        assert_eq!(mit(&["blende", "blende", "blende"]).arten(), 1);
        assert_eq!(mit(&["blende", "schnitt", "blende"]).arten(), 2);
    }

    #[test]
    fn die_erinnerung_waechst_nicht_unbegrenzt() {
        let mut v = Vielfalt::neu();
        for i in 0..100 {
            v.merken(&format!("art{i}"));
        }
        assert_eq!(v.letzte.len(), PLATZ);
        assert!(v.text().starts_with("art92"), "{}", v.text());
    }

    #[test]
    fn leeres_wird_nicht_gemerkt() {
        let mut v = Vielfalt::neu();
        v.merken("   ");
        assert!(v.ist_leer());
        assert_eq!(v.wiederholung(), 0);
    }

    /// Der Wechsel zählt, nicht der Wert — sonst wäre jede Blende acht
    /// Übergänge, weil eine Rampe achtmal schreibt.
    #[test]
    fn die_seite_unterscheidet_ankunft_von_bewegung() {
        assert_eq!(seite(1.0), 1);
        assert_eq!(seite(0.95), 1);
        assert_eq!(seite(-1.0), -1);
        assert_eq!(seite(0.0), 0);
        assert_eq!(seite(0.89), 0);
        // Von ganz links nach ganz rechts ist ein Wechsel, obwohl der Betrag
        // gleich bleibt.
        assert_ne!(seite(-1.0), seite(1.0));
    }

    #[test]
    fn ein_vorgemerkter_griff_hat_vorrang() {
        let mut v = Vielfalt::neu();
        v.vormerken("bassswap");
        v.angekommen("weich/32");
        assert_eq!(v.text(), "bassswap");
        // Und er gilt nur einmal.
        v.angekommen("weich/32");
        assert_eq!(v.text(), "bassswap, weich/32");
    }

    /// Ein Griff, der geplant und dann nicht gefahren wird, hat nicht
    /// stattgefunden — er darf die Zurückhaltung nicht beschönigen.
    #[test]
    fn ein_vorgemerkter_griff_ohne_ankunft_zaehlt_nicht() {
        let mut v = Vielfalt::neu();
        v.vormerken("filter");
        assert!(v.ist_leer());
        assert_eq!(v.wiederholung(), 0);
    }

    #[test]
    fn der_text_zeigt_das_muster() {
        assert_eq!(
            mit(&["blende", "blende", "schnitt"]).text(),
            "blende, blende, schnitt"
        );
    }
}
