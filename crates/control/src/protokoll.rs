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

/// Was eine Verbindung sich merkt.
///
/// Nur die Abonnements. Ohne sie müsste jeder Bediener pollen, und wer auf das
/// Ende eines Tracks wartet, pollt dann im Sekundentakt eine Anlage, die
/// gerade läuft.
#[derive(Default)]
pub struct Sitzung {
    abos: Vec<(Schluessel, Option<Wert>)>,
}

impl Sitzung {
    pub fn neu() -> Sitzung {
        Sitzung::default()
    }

    pub fn hat_abos(&self) -> bool {
        !self.abos.is_empty()
    }

    /// Was sich seit dem letzten Blick geändert hat.
    ///
    /// Ehrlich gesagt: Das Pult meldet nichts von sich aus, hier wird
    /// verglichen. Für Werte, die im Audio-Thread laufen — die Position ändert
    /// sich mit jedem Sample — wäre eine echte Benachrichtigung ohnehin
    /// sinnlos; man will sie in Abständen, nicht in Fluten. Der Gewinn liegt
    /// darin, dass der Bediener das nicht selbst bauen muss.
    pub fn aenderungen(&mut self, pult: &Steuerpult) -> Vec<String> {
        let mut zeilen = Vec::new();

        for (schluessel, letzter) in &mut self.abos {
            let Ok(jetzt) = pult.lies(schluessel) else {
                continue;
            };
            if letzter.as_ref() == Some(&jetzt) {
                continue;
            }
            zeilen.push(format!("value {schluessel} {jetzt}"));
            *letzter = Some(jetzt);
        }

        zeilen
    }
}

/// Beantwortet eine Zeile. Die Antwort kann mehrzeilig sein.
pub fn behandle(pult: &mut Steuerpult, sitzung: &mut Sitzung, zeile: &str) -> String {
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
        "do" => ausloesen(pult, erstes, zweites),
        "sub" => abonnieren(pult, sitzung, erstes, zweites),
        "unsub" => abbestellen(sitzung, erstes, zweites),
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
  do <control> [arg]     Aktion auslösen: sync, load, jump_cue, beatjump, search
  sub <control>...       Änderungen melden lassen, statt zu fragen
  unsub [control]...     Abbestellen; ohne Argument alles
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

        // Eine Spalte für „welche Werte sind erlaubt". Bei einer Aktion ist
        // das ihr Argument — ohne das wüsste ein Aufrufer nicht, dass `load`
        // einen Pfad will und `jump_cue` eine Zahl von 1 bis 8.
        let raum = match (b.art, b.bereich, b.auswahl) {
            (Art::Aktion, _, _) if !b.argument.is_empty() => b.argument.to_string(),
            (_, _, auswahl) if !auswahl.is_empty() => auswahl.join("|"),
            (_, Some((min, max)), _) if max == f64::MAX => format!("{min}.."),
            (_, Some((min, max)), _) => format!("{min}..{max}"),
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
        Art::Aktion => return format!("err {schluessel} ist eine Aktion — mit 'do' auslösen"),
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

/// Sammelt die Control-Namen einer Zeile ein.
fn namen(erstes: Option<&str>, rest: Option<&str>) -> Vec<String> {
    let mut aus = Vec::new();
    if let Some(e) = erstes {
        aus.push(e.to_string());
    }
    if let Some(r) = rest {
        aus.extend(r.split_whitespace().map(str::to_string));
    }
    aus
}

fn abonnieren(
    pult: &Steuerpult,
    sitzung: &mut Sitzung,
    erstes: Option<&str>,
    rest: Option<&str>,
) -> String {
    let gewuenscht = namen(erstes, rest);
    if gewuenscht.is_empty() {
        return "err sub braucht mindestens ein Control".to_string();
    }

    let mut angenommen = 0usize;
    for name in gewuenscht {
        let Some(schluessel) = Schluessel::parse(&name) else {
            return format!("err kein gültiger Name: {name}");
        };
        // Nur abonnieren, was sich auch lesen lässt — ein Abo auf eine Aktion
        // würde nie etwas melden und sähe trotzdem aus, als täte es das.
        if let Err(e) = pult.lies(&schluessel) {
            return format!("err {e}");
        }
        if sitzung.abos.iter().any(|(k, _)| k == &schluessel) {
            continue;
        }
        // Ohne letzten Wert: Der erste Vergleich meldet den Ist-Zustand, und
        // der Bediener muss nicht zusätzlich einmal `get` sagen.
        sitzung.abos.push((schluessel, None));
        angenommen += 1;
    }

    format!("ok sub {angenommen} neu, {} gesamt", sitzung.abos.len())
}

fn abbestellen(sitzung: &mut Sitzung, erstes: Option<&str>, rest: Option<&str>) -> String {
    let namen = namen(erstes, rest);
    if namen.is_empty() {
        let weg = sitzung.abos.len();
        sitzung.abos.clear();
        return format!("ok unsub {weg} abbestellt");
    }

    let vorher = sitzung.abos.len();
    for name in namen {
        sitzung.abos.retain(|(k, _)| k.to_string() != name);
    }
    format!(
        "ok unsub {} abbestellt, {} gesamt",
        vorher - sitzung.abos.len(),
        sitzung.abos.len()
    )
}

fn ausloesen(pult: &mut Steuerpult, control: Option<&str>, argument: Option<&str>) -> String {
    let Some(text) = control.filter(|t| !t.is_empty()) else {
        return "err do braucht eine Aktion".to_string();
    };
    let Some(schluessel) = Schluessel::parse(text) else {
        return format!("err kein gültiger Name: {text}");
    };

    match pult.ausloesen(&schluessel, argument) {
        Ok(zeilen) => {
            let mut aus = zeilen;
            aus.push(format!("ok {schluessel}"));
            aus.join("\n")
        }
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
        let mut s = Sitzung::neu();

        assert_eq!(
            behandle(&mut pult, &mut s, "set channel1.fader 0.8"),
            "ok channel1.fader 0.800000"
        );
        assert_eq!(
            behandle(&mut pult, &mut s, "get channel1.fader"),
            "value channel1.fader 0.800000"
        );
    }

    #[test]
    fn die_bestaetigung_zeigt_den_angekommenen_wert() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        // 9.0 wird auf 1.0 begrenzt — die Antwort sagt 1, nicht 9.
        assert_eq!(
            behandle(&mut pult, &mut s, "set channel1.fader 9"),
            "ok channel1.fader 1"
        );
    }

    #[test]
    fn schalter_verstehen_wort_und_zahl() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        for eingabe in ["1", "on", "an", "true"] {
            behandle(&mut pult, &mut s, "set deck1.play 0");
            assert_eq!(
                behandle(&mut pult, &mut s, &format!("set deck1.play {eingabe}")),
                "ok deck1.play 1",
                "{eingabe} hätte einschalten müssen"
            );
        }

        assert!(behandle(&mut pult, &mut s, "set deck1.play vielleicht").starts_with("err"));
    }

    #[test]
    fn list_zaehlt_auf_und_laesst_sich_filtern() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        let alles = behandle(&mut pult, &mut s, "list");
        assert!(alles.contains("control deck1.play schalter"));
        assert!(alles.contains("control master.crossfader"));

        let nur_deck2 = behandle(&mut pult, &mut s, "list deck2.");
        assert!(nur_deck2.contains("deck2.play"));
        assert!(!nur_deck2.contains("deck1.play"));
        assert!(!nur_deck2.contains("master."));
    }

    #[test]
    fn jede_zeile_der_liste_traegt_ihre_bedeutung() {
        // Der Unterschied zu Mixxx: Wer die Liste hat, braucht kein Handbuch.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        let antwort = behandle(&mut pult, &mut s, "list deck1.tempo");

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
        let mut s = Sitzung::neu();

        // Ein MIDI-Regler auf Anschlag: 127/127 = 1.0 normiert.
        assert_eq!(
            behandle(&mut pult, &mut s, "setn channel1.trim 1.0"),
            "ok channel1.trim 2"
        );
        // In der Raste bei einem bipolaren Control.
        assert_eq!(
            behandle(&mut pult, &mut s, "setn channel1.filter 0.5"),
            "ok channel1.filter 0"
        );
    }

    #[test]
    fn hot_cues_lassen_sich_ueber_das_protokoll_leeren() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        behandle(&mut pult, &mut s, "set deck1.cue1 8.25");
        assert_eq!(
            behandle(&mut pult, &mut s, "get deck1.cue1"),
            "value deck1.cue1 8.250000"
        );

        assert_eq!(
            behandle(&mut pult, &mut s, "set deck1.cue1 -"),
            "ok deck1.cue1 -"
        );
    }

    #[test]
    fn fehler_sind_immer_als_solche_erkennbar() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

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
            let antwort = behandle(&mut pult, &mut s, zeile);
            assert!(
                antwort.starts_with("err"),
                "'{zeile}' hätte einen Fehler geben müssen, gab: {antwort}"
            );
        }
    }

    #[test]
    fn leere_zeilen_erzeugen_kein_rauschen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        assert_eq!(behandle(&mut pult, &mut s, ""), "");
        assert_eq!(behandle(&mut pult, &mut s, "   "), "");
    }

    #[test]
    fn ueberzaehlige_leerzeichen_stoeren_nicht() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        assert_eq!(
            behandle(&mut pult, &mut s, "  set   channel1.fader   0.5  "),
            "ok channel1.fader 0.500000"
        );
    }
}

#[cfg(test)]
mod abo_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    #[test]
    fn ein_abo_meldet_zuerst_den_ist_zustand() {
        // Sonst müsste jeder Bediener nach dem Abonnieren noch einmal `get`
        // sagen, um überhaupt zu wissen, wo er steht.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        assert_eq!(
            behandle(&mut pult, &mut s, "sub deck1.play"),
            "ok sub 1 neu, 1 gesamt"
        );
        assert_eq!(s.aenderungen(&pult), vec!["value deck1.play 0"]);
    }

    #[test]
    fn gemeldet_wird_nur_was_sich_geaendert_hat() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "sub deck1.play channel1.fader");

        // Erster Durchgang: der Ist-Zustand beider.
        assert_eq!(s.aenderungen(&pult).len(), 2);
        // Zweiter ohne Änderung: nichts. Sonst wäre das Abo nur ein Poller
        // mit mehr Verkehr.
        assert!(s.aenderungen(&pult).is_empty());

        behandle(&mut pult, &mut s, "set channel1.fader 0.4");
        assert_eq!(s.aenderungen(&pult), vec!["value channel1.fader 0.400000"]);
    }

    #[test]
    fn mehrere_controls_in_einer_zeile() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        let antwort = behandle(&mut pult, &mut s, "sub deck1.play deck1.finished deck2.bpm");
        assert_eq!(antwort, "ok sub 3 neu, 3 gesamt");

        // Doppeltes Abonnieren zählt nicht doppelt.
        let nochmal = behandle(&mut pult, &mut s, "sub deck1.play");
        assert_eq!(nochmal, "ok sub 0 neu, 3 gesamt");
    }

    #[test]
    fn abbestellen_geht_einzeln_und_ganz() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "sub deck1.play deck2.bpm");

        assert_eq!(
            behandle(&mut pult, &mut s, "unsub deck1.play"),
            "ok unsub 1 abbestellt, 1 gesamt"
        );
        assert_eq!(
            behandle(&mut pult, &mut s, "unsub"),
            "ok unsub 1 abbestellt"
        );
        assert!(!s.hat_abos());
    }

    #[test]
    fn was_sich_nicht_lesen_laesst_wird_nicht_abonniert() {
        // Ein Abo auf eine Aktion würde nie etwas melden und sähe trotzdem
        // aus, als täte es das.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        assert!(behandle(&mut pult, &mut s, "sub deck1.sync").starts_with("err"));
        assert!(behandle(&mut pult, &mut s, "sub deck1.quatsch").starts_with("err"));
        assert!(!s.hat_abos());
    }

    #[test]
    fn aktionen_ueber_das_protokoll() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        let sync = behandle(&mut pult, &mut s, "do deck2.sync");
        assert!(sync.contains("sync deck2 auf deck1"), "{sync}");
        assert!(sync.ends_with("ok deck2.sync"), "{sync}");

        let suche = behandle(&mut pult, &mut s, "do master.search techno");
        assert!(
            suche.contains("track 128.00 8A /musik/techno-0.wav"),
            "{suche}"
        );

        // Aktion und Wert werden nicht verwechselt.
        assert!(behandle(&mut pult, &mut s, "set deck2.sync 1").starts_with("err"));
        assert!(behandle(&mut pult, &mut s, "do deck1.play").starts_with("err"));
        assert!(behandle(&mut pult, &mut s, "do").starts_with("err"));
    }

    #[test]
    fn ein_ladeauftrag_meldet_annahme_nicht_erledigung() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        let antwort = behandle(&mut pult, &mut s, "do deck1.load /musik/a.wav");
        assert!(antwort.contains("load deck1 angenommen"), "{antwort}");
        assert_eq!(
            behandle(&mut pult, &mut s, "get deck1.load_status"),
            "value deck1.load_status laedt"
        );
    }
}

#[cfg(test)]
mod katalog_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    /// Eine Aktion muss sagen, was sie erwartet.
    ///
    /// Ohne das steht in der Liste zwar, dass es `deck1.load` gibt, aber nicht,
    /// dass sie einen Pfad will — und wer nur die Liste hat, rät.
    #[test]
    fn aktionen_nennen_ihr_argument() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        let antwort = behandle(&mut pult, &mut s, "list deck1.load");
        assert!(
            antwort.contains("aktion <pfad>"),
            "das Argument fehlt: {antwort}"
        );

        let cue = behandle(&mut pult, &mut s, "list deck1.jump_cue");
        assert!(cue.contains("<1..8>"), "{cue}");

        // Eine Aktion ohne Argument steht als solche da.
        let stop = behandle(&mut pult, &mut s, "list master.record_stop");
        assert!(stop.contains("aktion -"), "{stop}");
    }
}
