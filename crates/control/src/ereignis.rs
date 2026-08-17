//! Was geschehen ist, ohne dass jemand danach gefragt hat.
//!
//! **Der Fund, mit dem P2 anfing.** Der Zeitplan meldet, wenn eine Rampe fertig
//! ist, abgebrochen wird oder — der interessante Fall — *abgelöst*, weil jemand
//! anders denselben Regler angefasst hat. Diese Meldungen gab der Taktgeber
//! zurück, und der Thread, der ihn aufruft, warf sie weg. Für einen einzelnen
//! Bediener fiel das nie auf: Er *war* derjenige, der den Regler angefasst hat.
//!
//! Für ein Team ist es der schlimmste denkbare Zustand. Zwei Agenten greifen
//! nach demselben Fader, einer verliert — und erfährt nichts. Er hält seine
//! Blende weiter für laufend, plant darauf auf, und der nächste Griff sitzt auf
//! einer Annahme, die seit zwanzig Sekunden falsch ist.
//!
//! Hier liegen diese Meldungen, bis jemand sie abholt.
//!
//! # Warum ein Ring mit Nummern und kein Wert
//!
//! Ein Control `master.last_event` wäre einfacher gewesen und falsch: Der
//! Taktgeber läuft alle 5 ms, der Server vergleicht alle 50 ms. Zwischen zwei
//! Blicken passen zehn Ereignisse, und neun davon wären weg — ausgerechnet in
//! dem Moment, in dem viel gleichzeitig geschieht, also genau dann, wenn man
//! sie braucht.
//!
//! Deshalb eine laufende Nummer je Zeile. Wer seine letzte kennt, bekommt alles
//! seither. Und wer zu langsam liest, **erfährt das**: Der Ring ist endlich, und
//! was herausfällt, wird gezählt statt verschwiegen — dieselbe Regel wie bei den
//! verworfenen Frames im Mitschnitt und den unlesbaren Zeilen der Mitschrift.
//! Eine Lücke, die aussieht wie keine, ist das schlechteste von allem.

use std::collections::VecDeque;

/// Wie viele Zeilen aufgehoben werden.
///
/// Bei 5 ms Takt sind 64 Zeilen gut drei Sekunden dichtes Geschehen — mehr, als
/// zwischen zwei Blicken eines Abonnenten (50 ms) je anfällt, und wenig genug,
/// dass niemand hier ein Protokoll suchen will. Wer alles will, liest die
/// Mitschrift.
pub const PLATZ: usize = 64;

/// Ereignisse mit laufender Nummer.
#[derive(Debug, Default)]
pub struct Ereignisse {
    zeilen: VecDeque<(u64, String)>,
    /// Die zuletzt vergebene Nummer. 0 heißt: noch nichts geschehen.
    nummer: u64,
}

impl Ereignisse {
    pub fn neu() -> Ereignisse {
        Ereignisse::default()
    }

    /// Die zuletzt vergebene Nummer.
    ///
    /// Wer hier einsteigt, merkt sie sich und bekommt danach alles Neue — die
    /// Vergangenheit ausdrücklich nicht. Ein Bediener, der sich gerade
    /// verbindet, soll nicht die Ablösungen von vor zehn Minuten nachgereicht
    /// bekommen.
    pub fn nummer(&self) -> u64 {
        self.nummer
    }

    pub fn letzte(&self) -> Option<&str> {
        self.zeilen.back().map(|(_, z)| z.as_str())
    }

    pub fn ist_leer(&self) -> bool {
        self.zeilen.is_empty()
    }

    /// Trägt ein Ereignis ein.
    ///
    /// Mehrzeiliges wird auf die erste Zeile gekürzt: Der Ring soll Bewegungen
    /// festhalten, keine Trefferlisten.
    pub fn melden(&mut self, zeile: &str) {
        let Some(erste) = zeile.lines().find(|z| !z.trim().is_empty()) else {
            return;
        };
        self.nummer += 1;
        self.zeilen
            .push_back((self.nummer, erste.trim().to_string()));
        while self.zeilen.len() > PLATZ {
            self.zeilen.pop_front();
        }
    }

    /// Alles, was nach `seit` dazugekommen ist.
    ///
    /// Zweiter Wert: wie viele Zeilen dazwischen aus dem Ring gefallen sind.
    /// Über null heißt, dass der Leser zu langsam war — und das gehört ihm
    /// gesagt, statt ihn glauben zu lassen, es sei nichts passiert.
    pub fn seit(&self, seit: u64) -> (Vec<String>, usize) {
        let verloren = match self.zeilen.front() {
            Some((aelteste, _)) if *aelteste > seit + 1 => (aelteste - seit - 1) as usize,
            _ => 0,
        };
        let neue = self
            .zeilen
            .iter()
            .filter(|(n, _)| *n > seit)
            .map(|(_, z)| z.clone())
            .collect();
        (neue, verloren)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mit(zeilen: &[&str]) -> Ereignisse {
        let mut e = Ereignisse::neu();
        for z in zeilen {
            e.melden(z);
        }
        e
    }

    #[test]
    fn wer_seine_nummer_kennt_bekommt_alles_seither() {
        let e = mit(&["plan 1 fertig", "plan 2 abgeloest", "plan 3 fertig"]);
        assert_eq!(e.nummer(), 3);

        let (neue, verloren) = e.seit(1);
        assert_eq!(neue, vec!["plan 2 abgeloest", "plan 3 fertig"]);
        assert_eq!(verloren, 0);

        assert_eq!(e.seit(3).0.len(), 0, "nichts Neues seit der letzten");
    }

    /// Wer sich verbindet, bekommt nicht die Vergangenheit nachgereicht.
    #[test]
    fn wer_jetzt_einsteigt_bekommt_nur_neues() {
        let mut e = mit(&["plan 1 fertig", "plan 2 fertig"]);
        let einstieg = e.nummer();
        e.melden("plan 3 abgeloest channel1.fader");

        assert_eq!(e.seit(einstieg).0, vec!["plan 3 abgeloest channel1.fader"]);
    }

    /// Der Ring ist endlich. Was herausfällt, wird **gezählt** — eine Lücke,
    /// die aussieht wie keine, wäre das schlechteste von allem.
    #[test]
    fn was_herausfaellt_wird_gezaehlt_statt_verschwiegen() {
        let mut e = Ereignisse::neu();
        for i in 0..PLATZ + 10 {
            e.melden(&format!("plan {i} fertig"));
        }

        let (neue, verloren) = e.seit(0);
        assert_eq!(neue.len(), PLATZ);
        assert_eq!(verloren, 10, "die Lücke wurde verschwiegen");

        // Wer mitgekommen ist, verliert nichts.
        let (_, verloren) = e.seit(e.nummer() - 1);
        assert_eq!(verloren, 0);
    }

    #[test]
    fn der_ring_waechst_nicht_unbegrenzt() {
        let mut e = Ereignisse::neu();
        for i in 0..1_000 {
            e.melden(&format!("plan {i} fertig"));
        }
        assert_eq!(e.zeilen.len(), PLATZ);
        assert_eq!(e.nummer(), 1_000);
    }

    #[test]
    fn eine_leere_meldung_bekommt_keine_nummer() {
        let mut e = Ereignisse::neu();
        e.melden("   \n  ");
        assert_eq!(e.nummer(), 0);
        assert!(e.ist_leer());
    }

    /// Mehrzeiliges wird gekürzt — der Ring hält Bewegungen fest, keine
    /// Trefferlisten.
    #[test]
    fn mehrzeiliges_wird_auf_die_erste_zeile_gekuerzt() {
        let mut e = Ereignisse::neu();
        e.melden("ok 3 Treffer\neins\nzwei");
        assert_eq!(e.letzte(), Some("ok 3 Treffer"));
        assert_eq!(e.nummer(), 1);
    }
}
