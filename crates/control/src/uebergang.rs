//! Ein Repertoire statt eines Handgriffs.
//!
//! **Der langweiligste DJ im Raum ist der, der immer dasselbe macht** — und
//! genau das war der erste automatisch gefahrene Übergang in diesem Projekt:
//! eine lange Blende, linear, jedes Mal. Sauber, messbar, seelenlos.
//!
//! Ein Mensch hat vier oder fünf Griffe im Kopf und wählt aus. Hier stehen sie
//! benannt: lange Blende, Bass-Swap, harter Schnitt auf die Eins,
//! Filter-Sweep, Schleifen-Ausstieg.
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
//! # Jeder Griff endet damit, dass das ausgehende Deck steht
//!
//! Das klang zuerst nach einer Kleinigkeit und war die Stelle, an der aus
//! einem Übergang ein *Set* wird. Bis dahin lief der ausgehende Track nach der
//! Blende weiter — unhörbar hinter dem geschlossenen Crossfader, aber laufend.
//! Der nächste `uebergang` wurde deshalb abgewiesen: „es laufen mehrere Decks",
//! und wer aus- und wer eingeht, kann dann niemand wissen.
//!
//! Am laufenden Programm fiel das beim zweiten Griff auf, nicht beim ersten.
//! Ein einzelner Übergang war nie das Ziel.
//!
//! # Getaktet wird nach dem ausgehenden Deck
//!
//! Ohne Angabe nimmt `in` das erste Deck mit Beatgrid — und das ist beim
//! zweiten Griff eines Sets ausgerechnet das gerade gestoppte. Ein stehendes
//! Deck hält den Plan an, also stand der ganze Übergang still: angenommen,
//! eingetragen, nie gefahren. Das ausgehende Deck läuft dagegen per
//! Definition, solange der Griff dauert.
//!
//! Aufgefallen ist auch das erst beim zweiten Griff am laufenden Programm.
//! Beim ersten läuft zufällig das richtige Deck.
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

/// Wie viele Beats zwischen dem Ende einer Bewegung und dem Stoppen des
/// ausgehenden Decks liegen.
///
/// Ein Beat, und der ist nötig. Endet eine Rampe genau in dem Takt, in dem ihr
/// Taktgeber-Deck stehenbleibt, bekommt sie ihren letzten Schritt nicht mehr:
/// Der Regler steht zwar am Ziel, aber der Auftrag bleibt für immer im Plan
/// stehen — sichtbar für jeden zweiten Bediener, und nicht mehr wegzubekommen,
/// weil das Deck nicht mehr läuft. Am laufenden Programm sah man ihn nach dem
/// zweiten Griff dort liegen.
///
/// Hörbar ist der eine Beat nicht: Der Crossfader ist längst drüben.
const NACHLAUF: f64 = 1.0;

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
    /// Den Ausgehenden in eine Schleife legen und darüber wechseln.
    ///
    /// Der einzige Griff, der dem Ausgehenden **Zeit gibt**: Ein Track, der in
    /// vier Beats zu Ende wäre, hält so noch eine ganze Phrase durch. Genau
    /// dafür setzt ein Mensch am Ende eines Stücks eine Schleife — nicht, um
    /// etwas zu wiederholen, sondern um nicht gehetzt wechseln zu müssen.
    Schleife,
}

impl Griff {
    pub fn name(&self) -> &'static str {
        match self {
            Griff::Blende => "blende",
            Griff::Bassswap => "bassswap",
            Griff::Schnitt => "schnitt",
            Griff::Filter => "filter",
            Griff::Schleife => "schleife",
        }
    }

    pub fn parse(text: &str) -> Option<Griff> {
        match text {
            "blende" => Some(Griff::Blende),
            "bassswap" => Some(Griff::Bassswap),
            "schnitt" => Some(Griff::Schnitt),
            "filter" => Some(Griff::Filter),
            "schleife" => Some(Griff::Schleife),
            _ => None,
        }
    }

    pub const ALLE: [Griff; 5] = [
        Griff::Blende,
        Griff::Bassswap,
        Griff::Schnitt,
        Griff::Filter,
        Griff::Schleife,
    ];

    /// Wie lang der Griff üblicherweise ist, in Beats.
    ///
    /// Keine Vorschrift, nur die Zahl, die man ohne weiteres Nachdenken nimmt:
    /// zwei Phrasen für eine Blende, eine für den Bass-Swap, keine für den
    /// Schnitt.
    ///
    /// Bei der Schleife ist die Zahl zugleich die Schleifenlänge, und deshalb
    /// eine ganze Phrase: Eine Schleife, die nicht auf der Phrase liegt,
    /// verschiebt die Eins und macht aus dem Griff einen Fehler.
    pub fn beats(&self) -> f64 {
        match self {
            Griff::Blende => 32.0,
            Griff::Bassswap => 16.0,
            Griff::Schnitt => 0.0,
            Griff::Filter => 16.0,
            Griff::Schleife => 16.0,
        }
    }

    pub fn beschreibung(&self) -> &'static str {
        match self {
            Griff::Blende => "Lange Blende über den Crossfader, weich verteilt",
            Griff::Bassswap => "Beide laufen in der Mitte, dann tauschen die Bässe",
            Griff::Schnitt => "Auf der Eins umschalten, ohne Übergang",
            Griff::Filter => "Dem Ausgehenden den Boden wegziehen, während der Neue kommt",
            Griff::Schleife => "Den Ausgehenden in eine Schleife legen und darüber wechseln",
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
    let daus = aus + 1;

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
            format!("in deck{daus} phrase set deck{dein}.play 1"),
            format!("in deck{daus} phrase ramp master.crossfader {ziel} {beats} weich deck{daus}"),
            format!(
                "in deck{daus} phrase+{} set deck{daus}.play 0",
                beats + NACHLAUF
            ),
        ],
        Griff::Bassswap => {
            // Erst der Bass des Eingehenden weg, dann beide in die Mitte, dort
            // tauschen die Bässe über die halbe Länge, und danach ganz hinüber.
            let halb = (beats / 2.0).max(1.0);
            vec![
                format!("set channel{kein}.eq_low 0"),
                format!("in deck{daus} phrase set deck{dein}.play 1"),
                format!("in deck{daus} phrase ramp master.crossfader 0 {beats} weich deck{daus}"),
                format!("in deck{daus} phrase+{beats} ramp channel{kaus}.eq_low 0 {halb} deck{daus}"),
                format!("in deck{daus} phrase+{beats} ramp channel{kein}.eq_low 1 {halb} deck{daus}"),
                format!(
                    "in deck{daus} phrase+{} ramp master.crossfader {ziel} {beats} weich deck{daus}",
                    beats + halb
                ),
                format!(
                    "in deck{daus} phrase+{} set deck{daus}.play 0",
                    beats + halb + beats + NACHLAUF
                ),
            ]
        }
        Griff::Schnitt => vec![
            format!("in deck{daus} phrase set deck{dein}.play 1"),
            format!("in deck{daus} phrase set master.crossfader {ziel}"),
            format!("in deck{daus} phrase set deck{daus}.play 0"),
        ],
        Griff::Filter => vec![
            format!("in deck{daus} phrase set deck{dein}.play 1"),
            format!("in deck{daus} phrase ramp master.crossfader {ziel} {beats} frueh deck{daus}"),
            // Hochpass auf dem Ausgehenden: Der Boden geht weg, während der
            // Neue ihn übernimmt. `spaet`, damit es erst spürbar wird, wenn
            // der andere schon trägt.
            format!("in deck{daus} phrase ramp channel{kaus}.filter 1 {beats} spaet deck{daus}"),
            format!(
                "in deck{daus} phrase+{} set deck{daus}.play 0",
                beats + NACHLAUF
            ),
            // Der Hochpass geht mit dem Deck wieder auf. Ein Kanalzug, der
            // beim nächsten Track noch gefiltert dasteht, ist eine
            // Überraschung, die niemand bestellt hat — und man sucht sie
            // lange, weil man den Regler nicht angefasst hat.
            format!(
                "in deck{daus} phrase+{} set channel{kaus}.filter 0",
                beats + NACHLAUF
            ),
        ],
        Griff::Schleife => {
            // Die Schleife wird auf der Phrasengrenze gesetzt und ist genau so
            // lang wie die Blende darüber: Der Ausgehende läuft sie einmal
            // durch, und wenn sie herum ist, ist der Wechsel fertig. Eine
            // Schleife, die länger steht als der Übergang, wiederholt hörbar —
            // und das ist der Fehler, den dieser Griff gerade vermeiden soll.
            //
            // **Alles nach dem Setzen hängt am eingehenden Deck.** Der Beat des
            // ausgehenden wiederholt sich ja gerade — daran getaktet fing die
            // Blende bei jedem Schleifendurchlauf von vorn an, und die
            // Auflösung der Schleife kam nie, weil ihr Beat nie über das Ende
            // der Schleife hinauskam. Am laufenden Programm sah man den
            // Crossfader sieben Mal hin und her fahren.
            //
            // Das setzt voraus, dass der Eingehende auf einer Phrasengrenze
            // steht — `do deckN.jump_entry` sorgt dafür, und wer von Hand
            // irgendwohin springt, sollte es hier ohnehin nicht tun.
            vec![
                format!("in deck{daus} phrase set deck{daus}.loop_beats {beats}"),
                format!("in deck{daus} phrase set deck{dein}.play 1"),
                format!(
                    "in deck{dein} phrase ramp master.crossfader {ziel} {beats} weich deck{dein}"
                ),
                // Gelöst, nicht vergessen: Ein Deck, das im Hintergrund
                // weiterschleift, ist beim nächsten Griff eine Überraschung.
                format!("in deck{dein} phrase+{beats} set deck{daus}.loop_active 0"),
                format!(
                    "in deck{dein} phrase+{} set deck{daus}.play 0",
                    beats + NACHLAUF
                ),
            ]
        }
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
                zeilen
                    .iter()
                    .any(|z| z.starts_with("in deck") && z.contains(" phrase")),
                "{}: keine Zeile auf der Phrase: {zeilen:?}",
                g.name()
            );
        }
    }

    /// **Getaktet wird nach dem ausgehenden Deck, nie „nach dem ersten mit
    /// Grid".**
    ///
    /// Ohne Angabe nimmt `in` das erste Deck mit Beatgrid — beim zweiten Griff
    /// eines Sets also das gerade gestoppte. Ein stehendes Deck hält den Plan
    /// an, und der ganze Übergang stand still: angenommen, eingetragen, nie
    /// gefahren. Am laufenden Programm fiel das beim zweiten Griff auf, beim
    /// ersten läuft zufällig das richtige Deck.
    #[test]
    fn keine_zeile_haengt_am_ersten_deck_mit_grid() {
        let (pult, _runner) = laufend();
        for g in Griff::ALLE {
            for zeile in zeilen(&pult, g, g.beats()).expect("Zeilen") {
                if !zeile.starts_with("in ") {
                    continue;
                }
                assert!(
                    zeile.starts_with("in deck"),
                    "{} überlässt den Takt dem Zufall: {zeile}",
                    g.name()
                );
            }
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
    /// **Jeder Griff endet damit, dass das ausgehende Deck steht.**
    ///
    /// Das ist die Stelle, an der aus einem Übergang ein *Set* wird. Bis dahin
    /// lief der ausgehende Track weiter — unhörbar hinter dem geschlossenen
    /// Crossfader, aber laufend —, und der nächste `uebergang` wurde deshalb
    /// abgewiesen: „es laufen mehrere Decks". Am laufenden Programm fiel das
    /// beim zweiten Griff auf, nicht beim ersten.
    #[test]
    fn jeder_griff_stoppt_das_ausgehende_deck() {
        let (pult, _runner) = laufend();
        for griff in Griff::ALLE {
            let zeilen = zeilen(&pult, griff, 16.0).expect("Zeilen");
            assert!(
                zeilen.iter().any(|z| z.ends_with("set deck1.play 0")),
                "{} lässt das ausgehende Deck laufen: {zeilen:?}",
                griff.name()
            );
            // Und zwar nicht sofort, sondern erst wenn der Wechsel durch ist —
            // ein Schnitt ausgenommen, der hat keine Strecke.
            let stopp = zeilen
                .iter()
                .find(|z| z.ends_with("set deck1.play 0"))
                .unwrap();
            if griff != Griff::Schnitt {
                assert!(
                    stopp.contains("phrase+"),
                    "{} stoppt zu früh: {stopp}",
                    griff.name()
                );
            }
        }
    }

    /// **Keine Bewegung endet im selben Takt, in dem ihr Deck stehenbleibt.**
    ///
    /// Sonst bekommt sie ihren letzten Schritt nicht mehr: Der Regler steht am
    /// Ziel, der Auftrag aber für immer im Plan — sichtbar für jeden zweiten
    /// Bediener und nicht mehr wegzubekommen, weil das Deck nicht mehr läuft.
    /// Am laufenden Programm lag er nach dem zweiten Griff dort.
    #[test]
    fn keine_bewegung_endet_im_takt_des_stopps() {
        let (pult, _runner) = laufend();
        for griff in Griff::ALLE {
            let beats = 16.0;
            let zeilen = zeilen(&pult, griff, beats).expect("Zeilen");

            let Some(stopp) = zeilen.iter().find(|z| z.ends_with(".play 0")) else {
                panic!("{} stoppt nicht: {zeilen:?}", griff.name());
            };
            let bei = versatz(stopp);

            for zeile in &zeilen {
                let Some(rest) = zeile.split(" ramp ").nth(1) else {
                    continue;
                };
                // `ramp <control> <ziel> <beats> …`
                let laenge: f64 = rest.split_whitespace().nth(2).unwrap().parse().unwrap();
                let ende = versatz(zeile) + laenge;
                assert!(
                    ende + 1e-9 < bei,
                    "{}: Rampe endet bei {ende}, gestoppt wird bei {bei} — {zeile}",
                    griff.name()
                );
            }
        }
    }

    /// Liest das `phrase+n` einer Zeile; ohne Versatz null.
    fn versatz(zeile: &str) -> f64 {
        match zeile.split("phrase+").nth(1) {
            Some(rest) => rest
                .split_whitespace()
                .next()
                .and_then(|z| z.parse().ok())
                .unwrap_or(0.0),
            None => 0.0,
        }
    }

    /// **Der einzige Griff, der dem Ausgehenden Zeit gibt.**
    ///
    /// Die Schleife liegt genau so lang wie die Blende darüber: Der Ausgehende
    /// läuft sie einmal durch, und wenn sie herum ist, ist der Wechsel fertig.
    /// Eine Schleife, die länger steht als der Übergang, wiederholt hörbar —
    /// und das ist der Fehler, den dieser Griff gerade vermeiden soll.
    #[test]
    fn die_schleife_haelt_den_ausgehenden_genau_solange_wie_der_wechsel_dauert() {
        let (pult, _runner) = laufend();
        let zeilen = zeilen(&pult, Griff::Schleife, 16.0).expect("Zeilen");

        assert!(
            zeilen
                .iter()
                .any(|z| z == "in deck1 phrase set deck1.loop_beats 16"),
            "die Schleife wird nicht gesetzt: {zeilen:?}"
        );
        assert!(
            zeilen
                .iter()
                .any(|z| z == "in deck2 phrase+16 set deck1.loop_active 0"),
            "die Schleife wird nicht gelöst: {zeilen:?}"
        );
        // Gesetzt wird sie auf dem ausgehenden Deck — auf dem eingehenden wäre
        // sie das Gegenteil von Zeit gewinnen.
        assert!(
            !zeilen.iter().any(|z| z.contains("deck2.loop")),
            "die Schleife liegt auf dem falschen Deck: {zeilen:?}"
        );
        // **Alles nach dem Setzen hängt am eingehenden Deck.** Der Beat des
        // schleifenden wiederholt sich; daran getaktet fängt die Blende bei
        // jedem Durchlauf von vorn an, und die Auflösung kommt nie.
        for zeile in &zeilen {
            if zeile.contains("loop_beats") || zeile.contains("play 1") {
                continue;
            }
            assert!(
                zeile.starts_with("in deck2 "),
                "hängt am schleifenden Deck: {zeile}"
            );
        }
    }

    /// Eine gesetzte und nie gelöste Schleife ist beim nächsten Griff eine
    /// Überraschung — und zwar eine, die man erst hört, wenn es zu spät ist.
    #[test]
    fn jede_gesetzte_schleife_wird_auch_wieder_geloest() {
        let (pult, _runner) = laufend();
        for griff in Griff::ALLE {
            let zeilen = zeilen(&pult, griff, 16.0).expect("Zeilen");
            let gesetzt = zeilen.iter().any(|z| z.contains(".loop_beats"));
            let geloest = zeilen.iter().any(|z| z.contains(".loop_active 0"));
            assert_eq!(
                gesetzt,
                geloest,
                "{}: gesetzt {gesetzt}, gelöst {geloest} — {zeilen:?}",
                griff.name()
            );
        }
    }

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
