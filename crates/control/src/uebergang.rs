//! Ein Repertoire statt eines Handgriffs.
//!
//! **Der langweiligste DJ im Raum ist der, der immer dasselbe macht** — und
//! genau das war der erste automatisch gefahrene Übergang in diesem Projekt:
//! eine lange Blende, linear, jedes Mal. Sauber, messbar, seelenlos.
//!
//! Ein Mensch hat vier oder fünf Griffe im Kopf und wählt aus. Hier stehen sie
//! benannt: lange Blende, Bass-Swap, harter Schnitt auf die Eins,
//! Filter-Sweep.
//!
//! # Was hier ausdrücklich *nicht* passiert
//!
//! **Die Anlage wählt nicht aus.** Welcher Griff passt, hängt daran, ob der
//! ausgehende Track ein Outro hat, der eingehende ein langes Intro, wie groß
//! der Energieunterschied ist — und vor allem daran, was vorher schon dreimal
//! gefahren wurde. Das ist eine Entscheidung mit Gründen, und die gehört
//! demjenigen, der sie begründen kann.
//!
//! Seit der Gliederung (S2) stehen die Zahlen dafür im Steuerraum: `section`,
//! `intro_beats`, `beats_to_outro`. Ein Agent liest sie, wählt und sagt warum.
//! Nähme die Anlage ihm das ab, verlöre das Set genau den Teil, um den es
//! diesem Projekt geht.
//!
//! # Warum die Griffe in Protokollzeilen gebaut sind
//!
//! Jeder Griff ist eine Handvoll gewöhnlicher Zeilen — `in phrase …`,
//! `ramp … weich` —, die durch denselben Weg laufen wie alles andere. Damit
//! taucht er im Plan auf, in der Mitschrift und bei den Ereignissen, ein
//! zweiter Bediener sieht ihn kommen, und `cancel` nimmt ihn zurück.
//!
//! Vor allem aber: **Es gibt nichts, was ein Agent nicht auch selbst hätte
//! tippen können.** Der Griff spart ihm acht Zeilen, keine Magie. Die Antwort
//! nennt jede davon, damit er sie lesen, ändern und beim nächsten Mal von Hand
//! anders setzen kann.

use crate::pult::{Fehler, Steuerpult};

/// Ein benannter Übergang.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Griff {
    /// Lange Blende über den Crossfader, weich verteilt.
    Blende,
    /// Beide laufen in der Mitte, und die Bässe tauschen.
    Bassswap,
    /// Auf der Eins umschalten, ohne Übergang.
    Schnitt,
    /// Dem Ausgehenden den Boden wegziehen, während der Neue kommt.
    Filter,
}

impl Griff {
    pub fn name(&self) -> &'static str {
        match self {
            Griff::Blende => "blende",
            Griff::Bassswap => "bassswap",
            Griff::Schnitt => "schnitt",
            Griff::Filter => "filter",
        }
    }

    pub fn parse(text: &str) -> Option<Griff> {
        match text {
            "blende" => Some(Griff::Blende),
            "bassswap" => Some(Griff::Bassswap),
            "schnitt" => Some(Griff::Schnitt),
            "filter" => Some(Griff::Filter),
            _ => None,
        }
    }

    pub const ALLE: [Griff; 4] = [
        Griff::Blende,
        Griff::Bassswap,
        Griff::Schnitt,
        Griff::Filter,
    ];

    /// Wie lang der Griff üblicherweise ist, in Beats.
    ///
    /// Keine Vorschrift, nur die Zahl, die man ohne weiteres Nachdenken nimmt:
    /// zwei Phrasen für eine Blende, eine für den Bass-Swap, keine für den
    /// Schnitt.
    pub fn beats(&self) -> f64 {
        match self {
            Griff::Blende => 32.0,
            Griff::Bassswap => 16.0,
            Griff::Schnitt => 0.0,
            Griff::Filter => 16.0,
        }
    }

    pub fn beschreibung(&self) -> &'static str {
        match self {
            Griff::Blende => "Lange Blende über den Crossfader, weich verteilt",
            Griff::Bassswap => "Beide laufen in der Mitte, dann tauschen die Bässe",
            Griff::Schnitt => "Auf der Eins umschalten, ohne Übergang",
            Griff::Filter => "Dem Ausgehenden den Boden wegziehen, während der Neue kommt",
        }
    }
}

/// Welche Decks gerade aus- und eingehen.
///
/// Ausgehend ist das Deck, das läuft; eingehend das andere, sofern etwas darauf
/// liegt. Mehr Raterei findet hier nicht statt — wer es anders will, tippt die
/// Zeilen selbst.
pub fn paar(pult: &Steuerpult) -> Result<(usize, usize), Fehler> {
    let laeuft: Vec<usize> = pult
        .decks()
        .iter()
        .enumerate()
        .filter(|(_, d)| d.state.is_playing())
        .map(|(i, _)| i)
        .collect();

    let aus = match laeuft.as_slice() {
        [] => {
            return Err(Fehler::Gescheitert(
                "kein Deck läuft — kein Übergang".into(),
            ))
        }
        [eins] => *eins,
        // Läuft schon beides, ist der Übergang längst im Gange, und welcher
        // von beiden gemeint ist, kann hier niemand wissen.
        _ => {
            return Err(Fehler::Gescheitert(
                "es laufen mehrere Decks — dann sind die Zeilen von Hand zu setzen".into(),
            ));
        }
    };

    let ein = pult
        .decks()
        .iter()
        .enumerate()
        .find(|(i, d)| *i != aus && d.frames > 0)
        .map(|(i, _)| i)
        .ok_or_else(|| {
            Fehler::Gescheitert("auf dem anderen Deck liegt nichts — erst laden".into())
        })?;

    Ok((aus, ein))
}

/// Die Zeilen, aus denen ein Griff besteht.
///
/// Sie laufen anschließend durch dasselbe Protokoll wie alles andere; hier
/// entsteht nur der Text.
pub fn zeilen(pult: &Steuerpult, griff: Griff, beats: f64) -> Result<Vec<String>, Fehler> {
    if beats < 0.0 {
        return Err(Fehler::Gescheitert(
            "eine Länge unter null gibt es nicht".into(),
        ));
    }
    let (aus, ein) = paar(pult)?;
    let kaus = pult.decks()[aus].kanal + 1;
    let kein = pult.decks()[ein].kanal + 1;
    let dein = ein + 1;

    // Der Crossfader steht auf −1 für A und +1 für B. Wohin er soll, sagt der
    // Kanalzug des eingehenden Decks, nicht die Deck-Nummer: Welche Seite ein
    // Zug bedient, ist einstellbar.
    let ziel = match pult
        .kanaele()
        .get(pult.decks()[ein].kanal)
        .map(|k| k.assign)
    {
        Some(audio_engine::Assign::A) => -1.0,
        Some(audio_engine::Assign::B) => 1.0,
        _ => {
            return Err(Fehler::Gescheitert(format!(
                "channel{kein} hängt nicht am Crossfader — kein Übergang darüber"
            )));
        }
    };

    Ok(match griff {
        Griff::Blende => vec![
            format!("in phrase set deck{dein}.play 1"),
            format!("in phrase ramp master.crossfader {ziel} {beats} weich"),
        ],
        Griff::Bassswap => {
            // Erst der Bass des Eingehenden weg, dann beide in die Mitte, dort
            // tauschen die Bässe über die halbe Länge, und danach ganz hinüber.
            let halb = (beats / 2.0).max(1.0);
            vec![
                format!("set channel{kein}.eq_low 0"),
                format!("in phrase set deck{dein}.play 1"),
                format!("in phrase ramp master.crossfader 0 {beats} weich"),
                format!("in phrase+{beats} ramp channel{kaus}.eq_low 0 {halb}"),
                format!("in phrase+{beats} ramp channel{kein}.eq_low 1 {halb}"),
                format!(
                    "in phrase+{} ramp master.crossfader {ziel} {beats} weich",
                    beats + halb
                ),
            ]
        }
        Griff::Schnitt => vec![
            format!("in phrase set deck{dein}.play 1"),
            format!("in phrase set master.crossfader {ziel}"),
        ],
        Griff::Filter => vec![
            format!("in phrase set deck{dein}.play 1"),
            format!("in phrase ramp master.crossfader {ziel} {beats} frueh"),
            // Hochpass auf dem Ausgehenden: Der Boden geht weg, während der
            // Neue ihn übernimmt. `spaet`, damit es erst spürbar wird, wenn
            // der andere schon trägt.
            format!("in phrase ramp channel{kaus}.filter 1 {beats} spaet"),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;
    use crate::wert::Wert;
    use crate::Schluessel;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    fn laufend() -> (Steuerpult, audio_engine::EngineRunner) {
        let (mut pult, runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();
        (pult, runner)
    }

    /// Ausgehend ist, was läuft; eingehend das andere.
    #[test]
    fn das_paar_ergibt_sich_daraus_wer_laeuft() {
        let (pult, _runner) = laufend();
        assert_eq!(paar(&pult).unwrap(), (0, 1));
    }

    /// Läuft nichts, gibt es keinen Übergang — und das wird gesagt, statt
    /// irgendein Deck zu wählen.
    #[test]
    fn ohne_laufendes_deck_gibt_es_keinen_uebergang() {
        let (pult, _runner) = pult_mit_zwei_decks();
        let fehler = paar(&pult).unwrap_err();
        assert!(format!("{fehler}").contains("kein Deck läuft"), "{fehler}");
    }

    /// Laufen schon beide, ist der Übergang im Gange. Wer dann gemeint ist,
    /// kann hier niemand wissen — also raten wir nicht.
    #[test]
    fn bei_zwei_laufenden_decks_wird_nicht_geraten() {
        let (mut pult, _runner) = laufend();
        pult.schreibe(&k("deck2.play"), Wert::Schalter(true))
            .unwrap();
        let fehler = paar(&pult).unwrap_err();
        assert!(format!("{fehler}").contains("mehrere"), "{fehler}");
    }

    /// **Jeder Griff fängt auf einer Phrasengrenze an.** Das ist der Punkt, an
    /// dem ein Übergang aufhört, irgendwo zu sitzen.
    #[test]
    fn jeder_griff_beginnt_auf_der_phrase() {
        let (pult, _runner) = laufend();
        for g in Griff::ALLE {
            let zeilen =
                zeilen(&pult, g, g.beats()).unwrap_or_else(|e| panic!("{}: {e}", g.name()));
            assert!(
                zeilen.iter().any(|z| z.starts_with("in phrase")),
                "{}: keine Zeile auf der Phrase: {zeilen:?}",
                g.name()
            );
        }
    }

    /// Und jeder startet das eingehende Deck — sonst blendet man auf Stille.
    /// Genau das ist beim ersten selbstgefahrenen Set passiert.
    #[test]
    fn jeder_griff_startet_das_eingehende_deck() {
        let (pult, _runner) = laufend();
        for g in Griff::ALLE {
            let zeilen =
                zeilen(&pult, g, g.beats()).unwrap_or_else(|e| panic!("{}: {e}", g.name()));
            assert!(
                zeilen.iter().any(|z| z.contains("set deck2.play 1")),
                "{}: das eingehende Deck bleibt stehen: {zeilen:?}",
                g.name()
            );
        }
    }

    /// Der Crossfader geht auf die Seite des eingehenden Kanalzugs, nicht auf
    /// die der Deck-Nummer: Welche Seite ein Zug bedient, ist einstellbar.
    #[test]
    fn der_crossfader_geht_zur_seite_des_eingehenden_zuges() {
        let (pult, _runner) = laufend();
        let zeilen = zeilen(&pult, Griff::Blende, 32.0).unwrap();
        assert!(
            zeilen.iter().any(|z| z.contains("master.crossfader 1")),
            "Deck 2 liegt auf B, also nach +1: {zeilen:?}"
        );
    }

    /// Der Bass-Swap ist der einzige Griff, bei dem beide gleichzeitig laut
    /// laufen — daran erkennt man ihn.
    #[test]
    fn der_bassswap_geht_ueber_die_mitte_und_tauscht_die_baesse() {
        let (pult, _runner) = laufend();
        let zeilen = zeilen(&pult, Griff::Bassswap, 16.0).unwrap();
        let text = zeilen.join("\n");
        assert!(text.contains("master.crossfader 0 "), "keine Mitte: {text}");
        assert!(text.contains("channel1.eq_low 0"), "der alte Bass bleibt");
        assert!(
            text.contains("channel2.eq_low 1"),
            "der neue Bass kommt nicht"
        );
        // Und der Neue kommt ohne Bass herein, sonst dröhnt es in der Mitte.
        assert!(
            zeilen[0].starts_with("set channel2.eq_low 0"),
            "der Eingehende kommt mit Bass: {:?}",
            zeilen[0]
        );
    }

    /// Ein Schnitt ist keine Blende: kein `ramp`, nur ein Umlegen.
    #[test]
    fn der_schnitt_rampt_nicht() {
        let (pult, _runner) = laufend();
        let zeilen = zeilen(&pult, Griff::Schnitt, 0.0).unwrap();
        assert!(
            !zeilen.iter().any(|z| z.contains("ramp")),
            "ein Schnitt mit Rampe ist kein Schnitt: {zeilen:?}"
        );
        assert!(zeilen.iter().any(|z| z.contains("set master.crossfader")));
    }

    #[test]
    fn jeder_griff_ueberlebt_seinen_namen() {
        for g in Griff::ALLE {
            assert_eq!(Griff::parse(g.name()), Some(g));
            assert!(!g.beschreibung().is_empty());
        }
        assert_eq!(Griff::parse("wumms"), None);
    }
}
