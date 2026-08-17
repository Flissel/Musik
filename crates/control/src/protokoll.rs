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

use crate::mitschrift::Richtung;
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

    // Vor dem Ausführen: Der festgehaltene Frame soll der sein, an dem der
    // Befehl ankam, nicht der, an dem die Antwort fertig war.
    if aendert(befehl) {
        pult.halten(Richtung::Befehl, zeile);
    }

    let antwort = match befehl {
        "list" => list(pult, erstes),
        "get" => get(pult, erstes),
        "set" => set(pult, erstes, zweites, false),
        "setn" => set(pult, erstes, zweites, true),
        "do" => ausloesen(pult, erstes, zweites),
        "sub" => abonnieren(pult, sitzung, erstes, zweites),
        "unsub" => abbestellen(sitzung, erstes, zweites),
        "ramp" => rampe(pult, erstes, zweites),
        "in" => spaeter(pult, erstes, zweites),
        "when" => wenn(pult, erstes, zweites),
        "plan" => plan_zeigen(pult),
        "cancel" => streichen(pult, erstes),
        "help" => HILFE.to_string(),
        andere => format!("err unbekannter Befehl: {andere} (help hilft)"),
    };

    if aendert(befehl) {
        // Auch die Antwort, denn sie trägt die Plannummer — ohne sie ließe
        // sich ein späteres „plan 3 fertig" keinem Auftrag zuordnen. Und ein
        // `err` gehört genauso hinein: Ein Befehl, der nicht durchkam, erklärt
        // im Nachhinein mehr als einer, der durchkam.
        pult.halten(Richtung::Meldung, &antwort);
    }

    antwort
}

/// Ob ein Befehl etwas verändert — und damit in die Mitschrift gehört.
///
/// `list`, `get`, `plan`, `help` und die Abos fragen nur nach. Sie stünden in
/// jeder zweiten Zeile und würden zuschütten, was die Mitschrift zeigen soll:
/// die Bewegungen. Ein Ereignisprotokoll, in dem man suchen muss, ist eines,
/// in das niemand sieht.
fn aendert(befehl: &str) -> bool {
    matches!(
        befehl,
        "set" | "setn" | "do" | "ramp" | "in" | "when" | "cancel"
    )
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
  ramp <control> <ziel> <beats> [deck]   Regler über Beats bewegen
  in <beats> <befehl>    Befehl nach so vielen Beats ausführen
  in phrase[+n] <befehl> Befehl auf der nächsten Phrasengrenze ausführen
  when <control> < <wert> <befehl>   Befehl, sobald ein Wert die Schwelle reisst
  plan              was vorgemerkt ist
  cancel [id]       Vorgemerktes zurücknehmen; ohne Argument alles
  help              diese Übersicht";

/// `ramp <control> <ziel> <beats> [deck]`
///
/// Der Verb, der aus Reglerstellungen einen Übergang macht. Ohne ihn müsste
/// ein Bediener die Bewegung selbst in Schritte zerlegen und dazwischen
/// schlafen — über eine Leitung, die dafür zu ungenau ist.
fn rampe(pult: &mut Steuerpult, control: Option<&str>, rest: Option<&str>) -> String {
    let Some(control) = control.and_then(Schluessel::parse) else {
        return "err ramp braucht ein Control: ramp <control> <ziel> <beats> [deck]".into();
    };
    let mut rest = rest.unwrap_or("");
    let ziel = wort(&mut rest);
    let beats = wort(&mut rest);
    let deck = wort(&mut rest);

    let (Ok(ziel), Ok(beats)) = (ziel.parse::<f64>(), beats.parse::<f64>()) else {
        return "err ramp braucht Ziel und Länge in Beats: ramp channel1.fader 0 16".into();
    };
    let takt_deck = match deck {
        "" => None,
        name => match crate::schluessel::Gruppe::parse(name) {
            Some(crate::schluessel::Gruppe::Deck(i)) => Some(i),
            _ => return format!("err {name} ist kein Deck"),
        },
    };

    let mut plan = std::mem::take(&mut pult.plan);
    let ergebnis =
        crate::zeitplan::rampe_planen(pult, &mut plan, control.clone(), ziel, beats, takt_deck);
    pult.plan = plan;

    match ergebnis {
        Ok(id) => format!("ok plan {id} ramp {control} nach {ziel} über {beats} Beats"),
        Err(e) => format!("err {e}"),
    }
}

/// `in <beats|phrase|phrase+n> <befehl>`
///
/// **`phrase` ist der Grund, warum es dieses Verb in dieser Form gibt.** Ein
/// Übergang beginnt auf der Eins einer Phrase, nicht nach einer runden Zahl
/// Beats. Wer das mit `in 32` nachbaut, trifft irgendwo hin — und der Mix
/// klingt danach, egal wie sauber alles andere sitzt.
fn spaeter(pult: &mut Steuerpult, beats: Option<&str>, befehl: Option<&str>) -> String {
    let beats = match beats_bezug(pult, beats.unwrap_or("")) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let Some(befehl) = befehl.filter(|b| !b.trim().is_empty()) else {
        return "err in braucht einen Befehl: in 16 set channel2.fader 0.9".into();
    };

    let mut plan = std::mem::take(&mut pult.plan);
    let ergebnis =
        crate::zeitplan::spaeter_planen(pult, &mut plan, beats, befehl.to_string(), None);
    pult.plan = plan;

    match ergebnis {
        Ok(id) => format!("ok plan {id} in {beats} Beats: {befehl}"),
        Err(e) => format!("err {e}"),
    }
}

/// `when <control> <\< oder \>> <wert> <befehl>`
///
/// Das Gegenstück zu `in`: Der eine wartet auf Takte, der andere auf einen
/// Zustand. „Wenn Deck A noch 32 Beats hat, leg den nächsten auf" ist die
/// Frage, die beim Auflegen wirklich gestellt wird — und ohne dieses Verb
/// müsste sie jemand durch Abonnieren und Nachrechnen beantworten.
fn wenn(pult: &mut Steuerpult, control: Option<&str>, rest: Option<&str>) -> String {
    let Some(control) = control.and_then(Schluessel::parse) else {
        return "err when braucht ein Control: when deck1.beats_left < 32 do master.queue_next"
            .into();
    };
    let mut rest = rest.unwrap_or("");
    let vergleich = wort(&mut rest);
    let schwelle = wort(&mut rest);
    let zeile = rest.trim();

    let Some(vergleich) = crate::zeitplan::Vergleich::parse(vergleich) else {
        return format!("err {vergleich} ist kein Vergleich — erlaubt sind < und >");
    };
    let Ok(schwelle) = schwelle.parse::<f64>() else {
        return format!("err {schwelle} ist keine Zahl");
    };
    if zeile.is_empty() {
        return "err when braucht einen Befehl: when deck1.beats_left < 32 do master.queue_next"
            .into();
    }

    let mut plan = std::mem::take(&mut pult.plan);
    let ergebnis = crate::zeitplan::wenn_planen(
        pult,
        &mut plan,
        control.clone(),
        vergleich,
        schwelle,
        zeile.to_string(),
    );
    pult.plan = plan;

    match ergebnis {
        Ok(id) => format!(
            "ok plan {id} wenn {control} {} {schwelle}: {zeile}",
            vergleich.zeichen()
        ),
        Err(e) => format!("err {e}"),
    }
}

/// Liest die Wartezeit eines `in`: eine Zahl, `phrase` oder `phrase+n`.
///
/// Der Bezugspunkt für `phrase` ist dasselbe Deck, auf dessen Takten der
/// Auftrag dann liegt — das erste mit Beatgrid. Sonst zählte man die Beats des
/// einen und die Phrase des anderen.
fn beats_bezug(pult: &Steuerpult, roh: &str) -> Result<f64, String> {
    if let Ok(zahl) = roh.parse::<f64>() {
        return Ok(zahl);
    }

    let (wort, versatz) = match roh.split_once('+') {
        Some((w, rest)) => {
            let Ok(v) = rest.parse::<f64>() else {
                return Err(format!("err {rest} ist keine Zahl Beats"));
            };
            (w, v)
        }
        None => (roh, 0.0),
    };
    if wort != "phrase" {
        return Err(
            "err in braucht Beats oder phrase: in 16 do deck2.sync, in phrase do deck2.sync".into(),
        );
    }

    let Some(deck) =
        (0..pult.decks().len()).find(|i| crate::zeitplan::beat_jetzt(pult, *i).is_some())
    else {
        return Err("err kein Deck mit Beatgrid — phrase hat keinen Bezugspunkt".into());
    };
    let Some(bis) = pult.beats_bis_phrase(deck) else {
        return Err("err die Phrasenlage ist nicht bekannt".into());
    };
    Ok(bis + versatz)
}

/// Was vorgemerkt ist — das gemeinsame Blatt, wenn mehrere bedienen.
fn plan_zeigen(pult: &Steuerpult) -> String {
    let mut zeilen = Vec::new();

    for a in pult.plan.auftraege() {
        let jetzt = crate::zeitplan::beat_jetzt(pult, a.takt_deck).unwrap_or(a.ab_beat);
        match &a.was {
            crate::zeitplan::Was::Rampe(r) => zeilen.push(format!(
                "plan {} ramp {} {:.4} → {:.4} über {} Beats, {:.1} gelaufen (deck{})",
                a.id,
                r.control,
                r.von,
                r.nach,
                r.beats,
                (jetzt - a.ab_beat).max(0.0),
                a.takt_deck + 1
            )),
            crate::zeitplan::Was::Spaeter { beim_beat, zeile } => zeilen.push(format!(
                "plan {} in {:.1} Beats: {zeile} (deck{})",
                a.id,
                (beim_beat - jetzt).max(0.0),
                a.takt_deck + 1
            )),
            // Mit dem Ist-Wert dahinter: Wer den Plan liest, will wissen, wie
            // weit die Schwelle noch weg ist, und nicht bloß, dass es sie gibt.
            crate::zeitplan::Was::Wenn {
                control,
                vergleich,
                schwelle,
                zeile,
            } => {
                let steht = match pult.lies(control) {
                    Ok(crate::wert::Wert::Zahl(v)) => format!("{v:.2}"),
                    _ => "?".into(),
                };
                zeilen.push(format!(
                    "plan {} wenn {control} {} {schwelle}: {zeile} (steht bei {steht})",
                    a.id,
                    vergleich.zeichen()
                ))
            }
        }
    }

    zeilen.push(format!("ok {} vorgemerkt", pult.plan.auftraege().len()));
    zeilen.join("\n")
}

fn streichen(pult: &mut Steuerpult, id: Option<&str>) -> String {
    let gewaehlt = match id {
        None => None,
        Some(text) => match text.parse::<u64>() {
            Ok(id) => Some(id),
            Err(_) => return format!("err {text} ist keine Plan-Nummer"),
        },
    };
    format!("ok {} gestrichen", pult.plan.streichen(gewaehlt))
}

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

#[cfg(test)]
mod plan_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    #[test]
    fn eine_rampe_laesst_sich_ueber_das_protokoll_vormerken() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        let antwort = behandle(&mut pult, &mut s, "ramp channel1.fader 0 16");
        assert!(
            antwort.starts_with("ok plan 1 ramp channel1.fader"),
            "{antwort}"
        );

        // Und steht danach im gemeinsamen Blatt.
        let plan = behandle(&mut pult, &mut s, "plan");
        assert!(plan.contains("plan 1 ramp channel1.fader"), "{plan}");
        assert!(plan.ends_with("ok 1 vorgemerkt"), "{plan}");
    }

    #[test]
    fn ein_befehl_laesst_sich_auf_beats_legen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        let antwort = behandle(&mut pult, &mut s, "in 32 do deck2.sync");
        assert!(
            antwort.starts_with("ok plan 1 in 32 Beats: do deck2.sync"),
            "{antwort}"
        );
        assert!(behandle(&mut pult, &mut s, "plan").contains("do deck2.sync"));
    }

    #[test]
    fn vorgemerktes_laesst_sich_zuruecknehmen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        behandle(&mut pult, &mut s, "ramp channel1.fader 0 16");
        behandle(&mut pult, &mut s, "ramp channel2.fader 1 16");

        assert_eq!(behandle(&mut pult, &mut s, "cancel 1"), "ok 1 gestrichen");
        assert_eq!(behandle(&mut pult, &mut s, "cancel"), "ok 1 gestrichen");
        assert!(behandle(&mut pult, &mut s, "plan").ends_with("ok 0 vorgemerkt"));
    }

    #[test]
    fn unsinnige_plaene_werden_benannt_abgewiesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        for zeile in [
            "ramp",
            "ramp channel1.fader",
            "ramp channel1.fader 0",
            "ramp channel1.fader 0 acht",
            "ramp deck1.play 1 8",
            "ramp channel1.fader 0 8 channel2",
            "in",
            "in 16",
            "in acht do deck2.sync",
            "cancel siebzehn",
        ] {
            let antwort = behandle(&mut pult, &mut s, zeile);
            assert!(antwort.starts_with("err"), "'{zeile}' → {antwort}");
        }
    }

    #[test]
    fn die_hilfe_nennt_die_neuen_verben() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        let hilfe = behandle(&mut pult, &mut s, "help");

        for verb in ["ramp", "in ", "plan", "cancel"] {
            assert!(hilfe.contains(verb), "{verb} fehlt in der Hilfe");
        }
    }
}

#[cfg(test)]
mod warteschlangen_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    fn mit_liste() -> (Steuerpult, audio_engine::EngineRunner, Sitzung) {
        let (mut pult, runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "do master.queue_add /musik/eins.wav");
        behandle(&mut pult, &mut s, "do master.queue_add /musik/zwei.wav");
        (pult, runner, s)
    }

    #[test]
    fn die_liste_wird_der_reihe_nach_abgearbeitet() {
        let (mut pult, _runner, mut s) = mit_liste();

        let liste = behandle(&mut pult, &mut s, "do master.queue");
        assert!(liste.contains("queue 1 /musik/eins.wav -"), "{liste}");
        assert!(liste.contains("queue 2 /musik/zwei.wav -"), "{liste}");

        let auf = behandle(&mut pult, &mut s, "do master.queue_next");
        assert!(auf.contains("queue 1 abgenommen /musik/eins.wav"), "{auf}");

        let rest = behandle(&mut pult, &mut s, "do master.queue");
        assert!(!rest.contains("eins.wav"), "{rest}");
        assert!(rest.contains("queue 2 /musik/zwei.wav"), "{rest}");
    }

    /// Der häufigste Zusammenstoß zweier Auswählender: Beide suchen, was zu
    /// 128 BPM in 8A passt, und finden denselben Track.
    #[test]
    fn derselbe_track_kommt_nicht_zweimal_in_die_liste() {
        let (mut pult, _runner, mut s) = mit_liste();

        let nochmal = behandle(&mut pult, &mut s, "do master.queue_add /musik/eins.wav");
        assert!(nochmal.starts_with("err"), "{nochmal}");
        assert!(
            nochmal.contains("Nummer 1"),
            "sagt nicht, wo er steht: {nochmal}"
        );
    }

    #[test]
    fn aufgelegt_wird_auf_ein_deck_das_nicht_laeuft() {
        let (mut pult, _runner, mut s) = mit_liste();
        behandle(&mut pult, &mut s, "set deck1.play 1");

        behandle(&mut pult, &mut s, "do master.queue_next");
        assert_eq!(
            behandle(&mut pult, &mut s, "get deck2.load_status"),
            "value deck2.load_status laedt",
            "der laufende Track wurde überschrieben"
        );
    }

    /// Laufen alle Decks, wird gefragt statt geraten — ein Track über einen
    /// laufenden zu legen, reißt den Mix ab.
    #[test]
    fn laufen_alle_decks_wird_gefragt_statt_geraten() {
        let (mut pult, _runner, mut s) = mit_liste();
        behandle(&mut pult, &mut s, "set deck1.play 1");
        behandle(&mut pult, &mut s, "set deck2.play 1");

        let antwort = behandle(&mut pult, &mut s, "do master.queue_next");
        assert!(antwort.starts_with("err"), "{antwort}");
        // Und der Eintrag steht noch da.
        assert!(behandle(&mut pult, &mut s, "do master.queue").contains("eins.wav"));

        // Mit ausdrücklicher Deckangabe geht es trotzdem.
        let mit_deck = behandle(&mut pult, &mut s, "do master.queue_next deck1");
        assert!(mit_deck.contains("abgenommen"), "{mit_deck}");
    }

    #[test]
    fn ein_gescheitertes_auflegen_verliert_den_eintrag_nicht() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        // Die Testsammlung weist alles ab, was nicht nach Audio aussieht.
        behandle(&mut pult, &mut s, "do master.queue_add /musik/kein.txt");

        let antwort = behandle(&mut pult, &mut s, "do master.queue_next");
        assert!(antwort.starts_with("err"), "{antwort}");
        assert!(
            behandle(&mut pult, &mut s, "do master.queue").contains("queue 1 /musik/kein.txt"),
            "der Eintrag ist verschwunden, ohne gespielt worden zu sein"
        );
    }

    #[test]
    fn eine_notiz_kommt_beim_auflegen_mit() {
        let (mut pult, _runner, mut s) = mit_liste();
        behandle(
            &mut pult,
            &mut s,
            "do master.queue_note 2 mehr Druck nach dem Break",
        );

        let liste = behandle(&mut pult, &mut s, "do master.queue");
        assert!(
            liste.contains("zwei.wav mehr Druck nach dem Break"),
            "{liste}"
        );

        behandle(&mut pult, &mut s, "do master.queue_bump 2");
        let auf = behandle(&mut pult, &mut s, "do master.queue_next");
        assert!(auf.contains("notiz mehr Druck nach dem Break"), "{auf}");
    }

    #[test]
    fn vorziehen_streichen_und_leeren() {
        let (mut pult, _runner, mut s) = mit_liste();

        assert!(behandle(&mut pult, &mut s, "do master.queue_bump 2").contains("Naechste"));
        assert!(behandle(&mut pult, &mut s, "do master.queue_drop 2").contains("gestrichen"));
        assert!(behandle(&mut pult, &mut s, "do master.queue").contains("eins.wav"));
        assert!(behandle(&mut pult, &mut s, "do master.queue_clear").contains("1 gestrichen"));
        assert!(behandle(&mut pult, &mut s, "do master.queue").contains("hinweis"));
    }

    #[test]
    fn unsinnige_listenbefehle_werden_benannt_abgewiesen() {
        let (mut pult, _runner, mut s) = mit_liste();

        for zeile in [
            "do master.queue_add",
            "do master.queue_drop",
            "do master.queue_drop zwei",
            "do master.queue_drop 99",
            "do master.queue_bump 99",
            "do master.queue_note",
            "do master.queue_note 2",
            "do master.queue_note 99 gibt es nicht",
            "do master.queue_next deck9",
            "do master.queue_next channel1",
        ] {
            let antwort = behandle(&mut pult, &mut s, zeile);
            assert!(antwort.starts_with("err"), "'{zeile}' → {antwort}");
        }
    }
}

#[cfg(test)]
mod wenn_tests {
    use super::*;
    use crate::testing::{pult_mit_zwei_decks, rendern, RATE};

    /// Der eigentliche Fall: „Wenn Deck A fast durch ist, leg den nächsten auf."
    ///
    /// Ohne dieses Verb müsste ein Bediener `beats_left` abonnieren, zwanzig
    /// Zahlen je Sekunde lesen und die Schwelle selbst heraussuchen — über eine
    /// Chat-Schnittstelle ist das nicht zu bezahlen.
    #[test]
    fn ein_befehl_wartet_auf_einen_wert() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "set deck1.play 1");

        // Der Testtrack ist 60 s lang, also 128 Beats bei 128 BPM.
        let antwort = behandle(
            &mut pult,
            &mut s,
            "when deck1.beats_left < 120 set channel2.fader 0.9",
        );
        assert!(
            antwort.starts_with("ok plan 1 wenn deck1.beats_left <"),
            "{antwort}"
        );

        let mut plan = std::mem::take(&mut pult.plan);

        // Nach einer Sekunde sind erst gut zwei Beats gelaufen — zu früh.
        let frueh = lauf(&mut pult, &mut plan, &mut runner, 1.0);
        assert!(frueh.is_empty(), "zu früh gelaufen: {frueh:?}");
        assert!(!plan.ist_leer());

        // Nach vier Sekunden sind es über acht Beats, die Schwelle ist gerissen.
        let spaet = lauf(&mut pult, &mut plan, &mut runner, 4.0);
        assert_eq!(spaet, vec!["set channel2.fader 0.9"]);
        assert!(plan.ist_leer(), "der Auftrag hängt noch im Plan");
    }

    /// Lässt die Anlage laufen und gibt zurück, was der Plan ausgeführt hat.
    fn lauf(
        pult: &mut Steuerpult,
        plan: &mut crate::Zeitplan,
        runner: &mut audio_engine::EngineRunner,
        sekunden: f64,
    ) -> Vec<String> {
        let mut gelaufen = Vec::new();
        for _ in 0..20 {
            rendern(runner, (RATE as f64 * sekunden / 20.0) as usize);
            crate::zeitplan::takt(pult, plan, &mut |_, zeile| {
                gelaufen.push(zeile.to_string());
                "ok".into()
            });
        }
        gelaufen
    }

    fn takten(pult: &mut Steuerpult, plan: &mut crate::Zeitplan) -> Vec<String> {
        let mut gelaufen = Vec::new();
        crate::zeitplan::takt(pult, plan, &mut |_, zeile| {
            gelaufen.push(zeile.to_string());
            "ok".into()
        });
        gelaufen
    }

    /// Trifft es schon zu, läuft es sofort — `when` heißt „sobald es so weit
    /// ist", nicht „beim nächsten Überschreiten".
    #[test]
    fn eine_schon_erfuellte_bedingung_loest_beim_naechsten_takt_aus() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(
            &mut pult,
            &mut s,
            "when deck1.beats_left < 999 do deck2.sync",
        );

        let mut plan = std::mem::take(&mut pult.plan);
        assert_eq!(takten(&mut pult, &mut plan), vec!["do deck2.sync"]);
    }

    /// Ein stehendes Deck hält den Takt an — ein `when` nicht, denn es hängt
    /// an einem Wert und nicht an Takten.
    #[test]
    fn ein_wenn_braucht_keinen_taktgeber() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        // Kein Deck läuft, und dieses hier hat nicht einmal ein Grid.
        behandle(&mut pult, &mut s, "set deck1.bpm_grid 0");
        behandle(&mut pult, &mut s, "when channel1.fader > 0.5 do deck2.sync");

        let mut plan = std::mem::take(&mut pult.plan);
        let ruhig = takten(&mut pult, &mut plan);
        assert!(ruhig.is_empty(), "{ruhig:?}");
        assert!(
            !plan.ist_leer(),
            "ohne Grid abgebrochen, obwohl es keins braucht"
        );

        pult.schreibe(
            &Schluessel::parse("channel1.fader").unwrap(),
            crate::wert::Wert::Zahl(0.9),
        )
        .unwrap();
        assert_eq!(takten(&mut pult, &mut plan), vec!["do deck2.sync"]);
    }

    #[test]
    fn unsinnige_bedingungen_werden_benannt_abgewiesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        for zeile in [
            "when",
            "when deck1.beats_left",
            "when deck1.beats_left < 32",
            "when deck1.beats_left ist 32 do deck2.sync",
            "when deck1.beats_left < viele do deck2.sync",
            "when deck1.quatsch < 32 do deck2.sync",
            // Ein Schalter lässt sich mit keiner Schwelle vergleichen. Ihn
            // anzunehmen hieße, einen Auftrag anzulegen, der stumm für immer
            // wartet — schlimmer als eine Absage.
            "when deck1.play < 1 do deck2.sync",
            "when deck1.title < 1 do deck2.sync",
        ] {
            let antwort = behandle(&mut pult, &mut s, zeile);
            assert!(antwort.starts_with("err"), "'{zeile}' → {antwort}");
        }
    }

    #[test]
    fn der_plan_zeigt_die_bedingung_mit_ist_wert() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "set channel1.fader 0.25");
        behandle(&mut pult, &mut s, "when channel1.fader > 0.8 do deck2.sync");

        let plan = behandle(&mut pult, &mut s, "plan");
        assert!(plan.contains("wenn channel1.fader > 0.8"), "{plan}");
        assert!(
            plan.contains("steht bei 0.25"),
            "der Ist-Wert fehlt: {plan}"
        );
    }

    #[test]
    fn die_hilfe_nennt_das_neue_verb() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        assert!(behandle(&mut pult, &mut s, "help").contains("when "));
    }
}

#[cfg(test)]
mod phrasen_tests {
    use super::*;
    use crate::testing::{pult_mit_zwei_decks, rendern};

    /// Der eigentliche Zweck: Ein Übergang beginnt auf der Eins einer Phrase.
    ///
    /// Mit `in 32` trifft man irgendwo hin, und der Mix klingt danach — egal
    /// wie sauber alles andere sitzt.
    #[test]
    fn in_phrase_legt_den_befehl_auf_die_naechste_eins() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "set deck1.play 1");

        // Vier Beats hinein: bis zur nächsten Sechzehnergruppe sind es zwölf.
        behandle(&mut pult, &mut s, "do deck1.beatjump 4");
        rendern(&mut runner, 1024);
        let bis = match pult.lies(&Schluessel::parse("deck1.beats_to_phrase").unwrap()) {
            Ok(crate::wert::Wert::Zahl(v)) => v,
            andere => panic!("{andere:?}"),
        };
        assert!((bis - 12.0).abs() < 0.2, "Vorbedingung: {bis}");

        let antwort = behandle(&mut pult, &mut s, "in phrase do deck2.sync");
        assert!(antwort.starts_with("ok plan 1 in 1"), "{antwort}");

        // Der Plan zählt dieselben Beats herunter, die beats_to_phrase nennt.
        let plan = behandle(&mut pult, &mut s, "plan");
        assert!(plan.contains("do deck2.sync"), "{plan}");
    }

    #[test]
    fn phrase_plus_versatz_liegt_eine_phrase_spaeter() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "set deck1.play 1");
        rendern(&mut runner, 1024);

        let ohne = behandle(&mut pult, &mut s, "in phrase do deck2.sync");
        let mit = behandle(&mut pult, &mut s, "in phrase+16 do deck2.sync");

        let zahl = |z: &str| -> f64 {
            z.split_whitespace()
                .nth(4)
                .and_then(|w| w.parse().ok())
                .unwrap_or(-1.0)
        };
        let (a, b) = (zahl(&ohne), zahl(&mit));
        assert!(
            (b - a - 16.0).abs() < 0.5,
            "erwartet 16 Beats Abstand, war {a} und {b}"
        );
    }

    /// Und der Punkt daran: Eine Rampe lässt sich damit auf die Eins legen,
    /// ohne dass `ramp` selbst etwas von Phrasen wissen müsste.
    #[test]
    fn eine_rampe_laesst_sich_auf_die_phrase_legen() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "set deck1.play 1");
        rendern(&mut runner, 1024);

        let antwort = behandle(&mut pult, &mut s, "in phrase ramp master.crossfader 1.0 32");
        assert!(antwort.starts_with("ok plan 1 in "), "{antwort}");
        assert!(
            antwort.contains("ramp master.crossfader 1.0 32"),
            "{antwort}"
        );
    }

    #[test]
    fn unsinnige_bezuege_werden_benannt_abgewiesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();

        for zeile in [
            "in takt do deck2.sync",
            "in phrase+viel do deck2.sync",
            "in phrasen do deck2.sync",
            "in phrase",
        ] {
            let antwort = behandle(&mut pult, &mut s, zeile);
            assert!(antwort.starts_with("err"), "'{zeile}' → {antwort}");
        }
    }

    /// Ohne Beatgrid gibt es keine Phrase — und das muss dastehen, statt
    /// stillschweigend auf null zu fallen.
    #[test]
    fn ohne_grid_hat_phrase_keinen_bezugspunkt() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        behandle(&mut pult, &mut s, "set deck1.bpm_grid 0");
        behandle(&mut pult, &mut s, "set deck2.bpm_grid 0");

        let antwort = behandle(&mut pult, &mut s, "in phrase do deck2.sync");
        assert!(antwort.starts_with("err"), "{antwort}");
        assert!(
            antwort.contains("Bezugspunkt") || antwort.contains("Beatgrid"),
            "{antwort}"
        );
    }

    #[test]
    fn die_hilfe_nennt_den_phrasenbezug() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let mut s = Sitzung::neu();
        assert!(behandle(&mut pult, &mut s, "help").contains("in phrase"));
    }
}
