//! Zeilenprotokoll für die Steuerung von außen.
//!
//! Bewusst Text und bewusst zeilenweise: Man kann es mit `nc` von Hand
//! sprechen, in einem Log lesen und ohne Bibliothek erzeugen. Ein
//! Binärprotokoll wäre schneller, aber hier fließen Reglerbewegungen, keine
//! Samples — ein paar Dutzend Zeilen je Sekunde im schlimmsten Fall.
//!
//! Die Trennung von der Steckdose ist Absicht: Hier drin kommt kein Socket vor,
//! nur Zeile rein, Zeile raus. Damit lässt sich das ganze Protokoll ohne
//! Netzwerk testen, und der Server daneben ist nur noch Verkabelung.
//!
//! ```text
//! > list
//! < control deck1.play schalter - rw Läuft das Deck
//! > get deck1.bpm
//! < value deck1.bpm 128.021
//! > set channel1.fader 0.8
//! < ok channel1.fader 0.8
//! > setn channel1.fader 1.0        (normiert, 0..1 — für MIDI)
//! < ok channel1.fader 1
//! ```

use crate::pult::Steuerpult;
use crate::schluessel::Schluessel;
use crate::wert::{Art, Wert};

/// Beantwortet eine Zeile. Die Antwort kann mehrzeilig sein.
pub fn behandle(pult: &mut Steuerpult, zeile: &str) -> String {
    let zeile = zeile.trim();
    if zeile.is_empty() {
        return String::new();
    }

    // Nicht `splitn`: Zwischen zwei Wörtern können mehrere Leerzeichen
    // stehen, und `splitn` macht daraus leere Felder. Der Rest bleibt am
    // Stück, damit ein Wert später auch einmal Leerzeichen enthalten darf.
    let mut rest = zeile;
    let befehl = wort(&mut rest);
    let erstes = Some(wort(&mut rest)).filter(|w| !w.is_empty());
    let zweites = Some(rest.trim()).filter(|w| !w.is_empty());

    match befehl {
        "list" => list(pult, erstes),
        "get" => get(pult, erstes),
        "set" => set(pult, erstes, zweites, false),
        "setn" => set(pult, erstes, zweites, true),
        "help" => HILFE.to_string(),
        andere => format!("err unbekannter Befehl: {andere} (help hilft)"),
    }
}

/// Nimmt das nächste Wort und lässt den Rest stehen.
fn wort<'a>(rest: &mut &'a str) -> &'a str {
    let getrimmt = rest.trim_start();
    match getrimmt.find(char::is_whitespace) {
        Some(ende) => {
            *rest = &getrimmt[ende..];
            &getrimmt[..ende]
        }
        None => {
            *rest = "";
            getrimmt
        }
    }
}

const HILFE: &str = "\
ok Befehle:
  list [praefix]    Controls aufzählen, gefiltert nach Namensanfang
  get <control>     Wert lesen
  set <control> <wert>   Wert setzen; '-' leert (Hot Cues)
  setn <control> <0..1>  Wert normiert setzen, für MIDI
  help              diese Übersicht";

fn list(pult: &Steuerpult, praefix: Option<&str>) -> String {
    let mut zeilen = Vec::new();

    for (schluessel, b) in pult.liste() {
        let name = schluessel.to_string();
        if let Some(p) = praefix {
            if !name.starts_with(p) {
                continue;
            }
        }

        // Bereich und Auswahl in eine Spalte: Beide sagen dasselbe — welche
        // Werte erlaubt sind.
        let raum = match (b.bereich, b.auswahl) {
            (_, auswahl) if !auswahl.is_empty() => auswahl.join("|"),
            (Some((min, max)), _) if max == f64::MAX => format!("{min}.."),
            (Some((min, max)), _) => format!("{min}..{max}"),
            _ => "-".to_string(),
        };

        zeilen.push(format!(
            "control {name} {} {raum} {} {} {}",
            b.art.name(),
            b.einheit.name(),
            if b.schreibbar { "rw" } else { "r" },
            b.text
        ));
    }

    if zeilen.is_empty() {
        return "ok 0 Controls".to_string();
    }

    let anzahl = zeilen.len();
    zeilen.push(format!("ok {anzahl} Controls"));
    zeilen.join("\n")
}

fn get(pult: &Steuerpult, control: Option<&str>) -> String {
    let Some(text) = control.filter(|t| !t.is_empty()) else {
        return "err get braucht ein Control".to_string();
    };
    let Some(schluessel) = Schluessel::parse(text) else {
        return format!("err kein gültiger Name: {text}");
    };

    match pult.lies(&schluessel) {
        Ok(wert) => format!("value {schluessel} {wert}"),
        Err(e) => format!("err {e}"),
    }
}

fn set(pult: &mut Steuerpult, control: Option<&str>, wert: Option<&str>, normiert: bool) -> String {
    let Some(text) = control.filter(|t| !t.is_empty()) else {
        return "err set braucht ein Control".to_string();
    };
    let Some(roh) = wert.filter(|t| !t.is_empty()) else {
        return format!("err set {text} braucht einen Wert");
    };
    let Some(schluessel) = Schluessel::parse(text) else {
        return format!("err kein gültiger Name: {text}");
    };

    if normiert {
        let Ok(norm) = roh.parse::<f64>() else {
            return format!("err {roh} ist keine Zahl");
        };
        return match pult.schreibe_normiert(&schluessel, norm) {
            Ok(()) => bestaetigen(pult, &schluessel),
            Err(e) => format!("err {e}"),
        };
    }

    let Some(b) = pult.beschreibung(&schluessel) else {
        return format!("err unbekanntes Control: {schluessel}");
    };

    let parsed = match b.art {
        Art::Schalter => match roh {
            "1" | "on" | "an" | "true" => Wert::Schalter(true),
            "0" | "off" | "aus" | "false" => Wert::Schalter(false),
            _ => return format!("err {schluessel} erwartet 0 oder 1, nicht {roh}"),
        },
        Art::Auswahl => Wert::Auswahl(roh.to_string()),
        Art::Text => Wert::Text(roh.to_string()),
        Art::Zahl => {
            if roh == "-" {
                // Leeren statt auf null setzen — bei einem Hot Cue ist das ein
                // Unterschied.
                Wert::Leer
            } else {
                match roh.parse::<f64>() {
                    Ok(v) => Wert::Zahl(v),
                    Err(_) => return format!("err {roh} ist keine Zahl"),
                }
            }
        }
    };

    match pult.schreibe(&schluessel, parsed) {
        Ok(()) => bestaetigen(pult, &schluessel),
        Err(e) => format!("err {e}"),
    }
}

/// Bestätigt mit dem Wert, der **wirklich** angekommen ist.
///
/// Nicht mit dem, der gesendet wurde: Begrenzung und Rundung können ihn
/// verändert haben, und wer steuert, soll das erfahren, ohne nachzufragen.
fn bestaetigen(pult: &Steuerpult, schluessel: &Schluessel) -> String {
    match pult.lies(schluessel) {
        Ok(wert) => format!("ok {schluessel} {wert}"),
        Err(e) => format!("err {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    #[test]
    fn setzen_und_lesen_ueber_das_protokoll() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        assert_eq!(
            behandle(&mut pult, "set channel1.fader 0.8"),
            "ok channel1.fader 0.800000"
        );
        assert_eq!(
            behandle(&mut pult, "get channel1.fader"),
            "value channel1.fader 0.800000"
        );
    }

    #[test]
    fn die_bestaetigung_zeigt_den_angekommenen_wert() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        // 9.0 wird auf 1.0 begrenzt — die Antwort sagt 1, nicht 9.
        assert_eq!(
            behandle(&mut pult, "set channel1.fader 9"),
            "ok channel1.fader 1"
        );
    }

    #[test]
    fn schalter_verstehen_wort_und_zahl() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        for eingabe in ["1", "on", "an", "true"] {
            behandle(&mut pult, "set deck1.play 0");
            assert_eq!(
                behandle(&mut pult, &format!("set deck1.play {eingabe}")),
                "ok deck1.play 1",
                "{eingabe} hätte einschalten müssen"
            );
        }

        assert!(behandle(&mut pult, "set deck1.play vielleicht").starts_with("err"));
    }

    #[test]
    fn list_zaehlt_auf_und_laesst_sich_filtern() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        let alles = behandle(&mut pult, "list");
        assert!(alles.contains("control deck1.play schalter"));
        assert!(alles.contains("control master.crossfader"));

        let nur_deck2 = behandle(&mut pult, "list deck2.");
        assert!(nur_deck2.contains("deck2.play"));
        assert!(!nur_deck2.contains("deck1.play"));
        assert!(!nur_deck2.contains("master."));
    }

    #[test]
    fn jede_zeile_der_liste_traegt_ihre_bedeutung() {
        // Der Unterschied zu Mixxx: Wer die Liste hat, braucht kein Handbuch.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let antwort = behandle(&mut pult, "list deck1.tempo");

        let zeile = antwort.lines().next().unwrap();
        assert!(zeile.contains("zahl"), "Typ fehlt: {zeile}");
        assert!(zeile.contains("0.92..1.08"), "Bereich fehlt: {zeile}");
        assert!(zeile.contains("faktor"), "Einheit fehlt: {zeile}");
        assert!(zeile.contains(" rw "), "Schreibbarkeit fehlt: {zeile}");
        assert!(
            zeile.contains("Originalgeschwindigkeit"),
            "Text fehlt: {zeile}"
        );
    }

    #[test]
    fn normiertes_setzen_ist_fuer_midi_da() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        // Ein MIDI-Regler auf Anschlag: 127/127 = 1.0 normiert.
        assert_eq!(
            behandle(&mut pult, "setn channel1.trim 1.0"),
            "ok channel1.trim 2"
        );
        // In der Raste bei einem bipolaren Control.
        assert_eq!(
            behandle(&mut pult, "setn channel1.filter 0.5"),
            "ok channel1.filter 0"
        );
    }

    #[test]
    fn hot_cues_lassen_sich_ueber_das_protokoll_leeren() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        behandle(&mut pult, "set deck1.cue1 8.25");
        assert_eq!(
            behandle(&mut pult, "get deck1.cue1"),
            "value deck1.cue1 8.250000"
        );

        assert_eq!(behandle(&mut pult, "set deck1.cue1 -"), "ok deck1.cue1 -");
    }

    #[test]
    fn fehler_sind_immer_als_solche_erkennbar() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        for zeile in [
            "quatsch",
            "get",
            "get deck1.quatsch",
            "get nichtsda.play",
            "set channel1.fader",
            "set channel1.fader laut",
            "set deck1.duration 5",
            "setn channel1.fader viel",
        ] {
            let antwort = behandle(&mut pult, zeile);
            assert!(
                antwort.starts_with("err"),
                "'{zeile}' hätte einen Fehler geben müssen, gab: {antwort}"
            );
        }
    }

    #[test]
    fn leere_zeilen_erzeugen_kein_rauschen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        assert_eq!(behandle(&mut pult, ""), "");
        assert_eq!(behandle(&mut pult, "   "), "");
    }

    #[test]
    fn ueberzaehlige_leerzeichen_stoeren_nicht() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        assert_eq!(
            behandle(&mut pult, "  set   channel1.fader   0.5  "),
            "ok channel1.fader 0.500000"
        );
    }
}
