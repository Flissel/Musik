//! N3 — die Schätzung des Kritikers gegen die Wahrheit der Anlage.
//!
//! Der Kritiker hört einen Mitschnitt und sagt, wann der Übergang begann. Die
//! Mitschrift weiß es. Einmal gemessen ergab das **3,7 Sekunden zu spät** — ein
//! Wert, aus dem sich nichts folgern lässt: Er könnte der Regelfall sein oder
//! ein Ausreißer, und ob er an der Länge der Blende hängt, am Griff oder am
//! Material, sagt eine einzelne Zahl nicht.
//!
//! Also viele Sets. Jedes wird **durch die echte Anlage gefahren** — echter
//! Mixer, echtes Pult, echtes Repertoire, echter Mitschnitt, echte Mitschrift
//! —, und dann bekommt der Kritiker die Datei, so wie er sie sonst bekommt.
//! Die Wahrheit ist der Frame, in dem der Crossfader zum ersten Mal bewegt
//! wurde; sie steht in der Mitschrift, weil die Anlage sie dort hinschreibt.
//!
//! # Was das nicht ist
//!
//! **Keine Aussage über Musik.** Das Material ist gebaut: Kick, Bass, ein
//! Akkord. Zwei Stücke unterscheiden sich in der Harmonik deutlicher, als
//! zwei Tracks einer Platte es oft tun — und der Kritiker sucht genau danach.
//! Die Streuung, die hier herauskommt, ist deshalb eher die **untere Schranke**
//! des Fehlers als sein Erwartungswert.
//!
//! Was sie trotzdem trägt: ob der Fehler ein Vorzeichen hat, ob er mit der
//! Länge des Übergangs wächst, und welcher Griff dem Kritiker Mühe macht. Das
//! sind Aussagen über das Verfahren, und dafür reicht gebautes Material.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};

use analysis::wechsel;
use audio_core::deck::{DeckState, Voice};
use audio_core::{Beatgrid, Track};
use audio_engine::{Assign, DeckSource, Engine, EngineRunner, SilentSource};
use control::mitschrift;
use control::protokoll::{Sitzung, behandle};
use control::pult::{DeckEintrag, KanalSpiegel, Steuerpult};
use control::zeitplan::takt;

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

/// Wie lange Deck 1 allein läuft, bevor der Griff kommt.
///
/// Der Kritiker vergleicht mit dem Klangbild von vor [`wechsel::ABSTAND_SEK`].
/// Läge der Übergang früher, vergliche er ihn mit dem Anlauf statt mit dem
/// ausgehenden Track — der Fehler wäre dann einer des Aufbaus, nicht des
/// Verfahrens.
const VORLAUF_SEK: f64 = 24.0;

/// Die Auflösung des Kritikers: ein Fenster.
///
/// Genauer als das kann keine Schätzung sein, und eine Unschärfe unter einem
/// Fenster ist deshalb keine Aussage.
const FENSTER: f64 = wechsel::FENSTER_SEK;

/// Wie lange nach dem Griff weitergelaufen wird.
const NACHLAUF_SEK: f64 = 20.0;

/// Ein Fall: ein Griff über so viele Beats, bei diesem Tempo.
struct Fall {
    griff: &'static str,
    beats: u32,
    bpm: f64,
    /// Halbtöne, um die der eingehende Track verschoben ist.
    ///
    /// Bei 0 wäre der Kritiker blind — er sucht eine Änderung der Harmonik.
    /// Sieben Halbtöne sind eine Quinte, also der harmonisch *nächste*
    /// Nachbar: der schwerste Fall, den ein DJ absichtlich herstellt.
    versatz: i32,
}

/// Das Ergebnis eines Falls.
struct Messung {
    fall: usize,
    wahrheit: f64,
    geschaetzt: Option<f64>,
    unschaerfe: f64,
}

impl Messung {
    fn fehler(&self) -> Option<f64> {
        self.geschaetzt.map(|g| g - self.wahrheit)
    }
}

fn faelle() -> Vec<Fall> {
    let mut aus = Vec::new();
    for (griff, beats) in [
        ("schnitt", 0u32),
        ("bassswap", 16),
        ("filter", 16),
        ("blende", 16),
        ("blende", 32),
        ("blende", 64),
    ] {
        for (bpm, versatz) in [(128.0, 5), (128.0, 7), (100.0, 5), (140.0, 7)] {
            aus.push(Fall {
                griff,
                beats,
                bpm,
                versatz,
            });
        }
    }
    aus
}

fn main() -> Result<()> {
    let ordner = argumente()?;
    std::fs::create_dir_all(&ordner)
        .with_context(|| format!("{} ließ sich nicht anlegen", ordner.display()))?;

    let liste = faelle();
    println!(
        "musik-streuung — {} Sets durch die echte Anlage\n",
        liste.len()
    );

    let mut messungen = Vec::new();
    for (i, fall) in liste.iter().enumerate() {
        let wav = ordner.join(format!("set{i:02}.wav"));
        print!(
            "  {:2}/{}  {} über {} Beats bei {:.0} BPM, +{} Halbtöne … ",
            i + 1,
            liste.len(),
            fall.griff,
            fall.beats,
            fall.bpm,
            fall.versatz
        );
        use std::io::Write;
        std::io::stdout().flush().ok();

        match fahren(fall, &wav) {
            Ok(wahrheit) => {
                let m = messen(i, &wav, wahrheit)?;
                match m.fehler() {
                    Some(f) => println!("Wahrheit {wahrheit:.2} s, geschätzt {:+.2} s", f),
                    None => println!("Wahrheit {wahrheit:.2} s, **nicht gefunden**"),
                }
                messungen.push(m);
            }
            Err(e) => println!("kein Set: {e}"),
        }
    }

    bericht(&liste, &messungen);
    Ok(())
}

/// Fährt ein Set durch die echte Anlage und gibt die Wahrheit zurück.
///
/// Die Wahrheit ist der Zeitpunkt, an dem der Crossfader **zum ersten Mal**
/// bewegt wurde — nicht der, an dem der Befehl kam. Zwischen beiden liegt bei
/// jedem Griff außer dem Schnitt eine Phrase Wartezeit, und der Kritiker kann
/// vom Befehl nichts wissen: Er hört nur, was am Regler geschah.
fn fahren(fall: &Fall, wav: &Path) -> Result<f64> {
    let (mut pult, mut runner) = anlage(fall)?;

    // Erst der Mitschnitt, dann die Musik: Was vor dem Start läuft, steht in
    // keiner Datei, und die Zeitrechnung der Mitschrift beginnt hier.
    fuehren(&mut pult, &format!("do master.record {}", wav.display()))?;
    for zeile in [
        "set channel1.fader 1",
        "set channel2.fader 1",
        "set master.crossfader -1",
        "set deck1.play 1",
    ] {
        fuehren(&mut pult, zeile)?;
    }

    laufen(&mut pult, &mut runner, VORLAUF_SEK);
    fuehren(
        &mut pult,
        &format!("do master.uebergang {} {}", fall.griff, fall.beats),
    )?;

    // So lange, dass auch die längste Blende samt Nachlauf durch ist.
    let dauer = fall.beats as f64 * 60.0 / fall.bpm + NACHLAUF_SEK;
    laufen(&mut pult, &mut runner, dauer);
    // **Ohne diese Prüfung misst der ganze Prüfstand Unsinn.** Beim ersten
    // Lauf rannte die Schleife hier so viel schneller als die Wiedergabe, dass
    // der Ring überlief: 1,2 von 2,1 Millionen Frames verworfen, also mehr als
    // die Hälfte des Mitschnitts. Die Datei war trotzdem lesbar, die Mitschrift
    // passte zu ihr, und die Zahlen sahen aus wie Messwerte — sie waren keine.
    let verworfen = zahl(&fuehren(&mut pult, "get master.record_dropped")?);
    if verworfen > 0.0 {
        bail!("{verworfen:.0} Frames verworfen — der Mitschnitt hat Lücken");
    }
    fuehren(&mut pult, "do master.record_stop")?;

    // Der Schreiber-Thread hängt am Mitschnitt-Ring; ohne ein paar Blöcke
    // mehr wäre die Datei noch offen, wenn wir sie gleich lesen.
    laufen(&mut pult, &mut runner, 0.5);
    drop(pult);
    drop(runner);
    warten_auf(wav)?;

    let protokoll = mitschrift::lesen(&wav.with_extension(mitschrift::ENDUNG))
        .map_err(|e| anyhow::anyhow!("Mitschrift: {e}"))?;
    erste_bewegung(&protokoll).context("in der Mitschrift bewegte sich der Crossfader nie")
}

/// Wann sich der Crossfader **wirklich bewegt hat**, in Sekunden.
///
/// Drei Zeilen sprechen in der Mitschrift vom Crossfader, und nur eine davon
/// ist gemeint:
///
/// ```text
/// 0       0.000  > set master.crossfader -1                     ← das Einrichten
/// 1152000 24.000 > in deck1 phrase ramp master.crossfader 1 16  ← die Bestellung
/// 1440000 30.000 > ramp master.crossfader 1 16 weich deck1      ← die Bewegung
/// ```
///
/// Beide Irrwege sind gegangen worden. Das Einrichten ergab Sekunde 0 und
/// damit einen Abstand zum Dateianfang. Die Bestellung sah richtig aus — 24,00
/// Sekunden, sauber, überall gleich — und war es nicht: Zwischen ihr und der
/// Bewegung liegt die Wartezeit auf die Phrase, hier sechs Sekunden. Gemessen
/// hätte man dem Kritiker vier Sekunden Verspätung angeschrieben, die in
/// Wahrheit sein Vorsprung waren.
///
/// Die Bewegung erkennt man daran, dass die Zeile **mit dem Befehl anfängt**.
/// Was mit `in` beginnt, ist eine Bestellung; was der Plan quittiert, geht in
/// die andere Richtung.
fn erste_bewegung(p: &mitschrift::Protokoll) -> Option<f64> {
    let ab = p
        .ereignisse
        .iter()
        .position(|e| e.text.contains("uebergang"))?;
    p.ereignisse
        .iter()
        .skip(ab)
        .find(|e| {
            e.text.starts_with("set master.crossfader")
                || e.text.starts_with("ramp master.crossfader")
        })
        .map(|e| e.sekunden(p.kopf.rate))
}

/// Lässt den Kritiker über den Mitschnitt gehen.
fn messen(fall: usize, wav: &Path, wahrheit: f64) -> Result<Messung> {
    let track = Track::decode_file(wav)
        .with_context(|| format!("{} ließ sich nicht lesen", wav.display()))?;
    let gefunden = wechsel::finden(&track);

    // Der nächstgelegene — nicht der erste. Ein Set kann mehr als einen
    // Ausschlag haben, und der Kritiker weiß nicht, welcher gemeint ist.
    let naechster = gefunden.iter().min_by(|a, b| {
        (a.beginn - wahrheit)
            .abs()
            .total_cmp(&(b.beginn - wahrheit).abs())
    });

    Ok(Messung {
        fall,
        wahrheit,
        geschaetzt: naechster.map(|u| u.beginn),
        unschaerfe: naechster.map(|u| u.unschaerfe).unwrap_or(0.0),
    })
}

fn bericht(faelle: &[Fall], messungen: &[Messung]) {
    let mit: Vec<&Messung> = messungen.iter().filter(|m| m.fehler().is_some()).collect();
    println!("\n── Die Verteilung ───────────────────────────");
    println!(
        "  {} von {} Übergängen hat der Kritiker überhaupt gefunden.",
        mit.len(),
        messungen.len()
    );
    if mit.is_empty() {
        println!("  Ohne Fund keine Verteilung.");
        return;
    }

    let mut fehler: Vec<f64> = mit.iter().filter_map(|m| m.fehler()).collect();
    fehler.sort_by(|a, b| a.total_cmp(b));
    let median = fehler[fehler.len() / 2];
    let mittel = fehler.iter().sum::<f64>() / fehler.len() as f64;
    let betrag = fehler.iter().map(|f| f.abs()).sum::<f64>() / fehler.len() as f64;

    println!("  Median {median:+.2} s, Mittel {mittel:+.2} s, im Betrag {betrag:.2} s");
    println!(
        "  Spanne {:+.2} bis {:+.2} s",
        fehler.first().copied().unwrap_or(0.0),
        fehler.last().copied().unwrap_or(0.0)
    );
    println!("  Ein Minus heißt: zu früh geschätzt, ein Plus: zu spät.");

    // Der Kritiker gibt zu jedem Fund eine Unschärfe an. Ob sie etwas wert
    // ist, entscheidet sich hier: Eine Fehlerspanne, die den Fehler nicht
    // deckt, ist schlimmer als keine — sie beruhigt.
    let gedeckt = mit
        .iter()
        .filter(|m| {
            m.fehler()
                .is_some_and(|f| f.abs() <= m.unschaerfe.max(FENSTER))
        })
        .count();
    println!(
        "  Die angegebene Unschärfe deckt den Fehler in {gedeckt} von {} Fällen.",
        mit.len()
    );

    let fehlend = messungen.len() - mit.len();
    if fehlend > 0 {
        println!("\n  {fehlend} Übergänge blieben unbemerkt. Der Kritiker vergleicht mit dem");
        println!(
            "  Klangbild von vor {:.0} s — dauert eine Blende länger, liegt der Vergleich",
            wechsel::ABSTAND_SEK
        );
        println!("  selbst schon mitten in ihr, und der Unterschied reißt die Schwelle nie.");
    }

    println!("\n── Nach Länge des Übergangs ─────────────────");
    let mut laengen: Vec<u32> = faelle.iter().map(|f| f.beats).collect();
    laengen.sort_unstable();
    laengen.dedup();
    for beats in laengen {
        let teil: Vec<f64> = mit
            .iter()
            .filter(|m| faelle[m.fall].beats == beats)
            .filter_map(|m| m.fehler())
            .collect();
        if teil.is_empty() {
            println!("  {beats:>2} Beats: nichts gefunden");
            continue;
        }
        let mittel = teil.iter().sum::<f64>() / teil.len() as f64;
        let unschaerfe: f64 = mit
            .iter()
            .filter(|m| faelle[m.fall].beats == beats)
            .map(|m| m.unschaerfe)
            .sum::<f64>()
            / teil.len() as f64;
        println!(
            "  {beats:>2} Beats: {mittel:+.2} s im Mittel über {} Sets, Unschärfe {unschaerfe:.1} s",
            teil.len()
        );
    }

    println!("\n── Nach Griff ───────────────────────────────");
    let mut griffe: Vec<&str> = faelle.iter().map(|f| f.griff).collect();
    griffe.sort_unstable();
    griffe.dedup();
    for griff in griffe {
        let teil: Vec<f64> = mit
            .iter()
            .filter(|m| faelle[m.fall].griff == griff)
            .filter_map(|m| m.fehler())
            .collect();
        let gesamt = faelle.iter().filter(|f| f.griff == griff).count();
        if teil.is_empty() {
            println!("  {griff:<9}: 0 von {gesamt} gefunden");
            continue;
        }
        let mittel = teil.iter().sum::<f64>() / teil.len() as f64;
        println!(
            "  {griff:<9}: {mittel:+.2} s im Mittel, {} von {gesamt} gefunden",
            teil.len()
        );
    }
}

/// Baut die Anlage: zwei Decks mit echtem Material, Mixer, Pult, Mitschnitt.
fn anlage(fall: &Fall) -> Result<(Steuerpult, EngineRunner)> {
    let mut engine = Engine::new(RATE as f32);
    let laenge = VORLAUF_SEK + fall.beats as f64 * 60.0 / fall.bpm + NACHLAUF_SEK + 30.0;

    let mut kanaele = Vec::new();
    let mut zustaende = Vec::new();
    for (name, assign, versatz) in [
        ("DECK A", Assign::A, 0),
        ("DECK B", Assign::B, fall.versatz),
    ] {
        let state = Arc::new(DeckState::new());
        let track = Arc::new(material(fall.bpm, versatz, laenge));
        let voice = Voice::new(track, Arc::clone(&state));
        let kanal = engine.add_channel(name, Box::new(DeckSource::new(voice)));
        engine.channel(kanal).set_assign(assign);
        kanaele.push((name, kanal, assign));
        zustaende.push(state);
    }
    let aux = engine.add_channel("AUX", Box::new(SilentSource));
    engine.channel(aux).set_assign(Assign::Thru);

    let (tap, aufnahme) = audio_engine::mitschnitt(RATE);
    engine.set_mitschnitt(tap);

    let (handle, runner) = audio_engine::engine_channel(engine, 256);
    let mut pult = Steuerpult::neu(handle);

    for ((name, kanal, assign), state) in kanaele.iter().zip(zustaende) {
        pult.kanal_hinzufuegen(KanalSpiegel::neu(*name, *assign));
        let mut eintrag = DeckEintrag::neu(state, *kanal, RATE);
        eintrag.frames = (RATE as f64 * laenge) as u64;
        eintrag.titel = name.to_string();
        pult.deck_hinzufuegen(eintrag);
    }
    pult.kanal_hinzufuegen(KanalSpiegel::neu("AUX", Assign::Thru));

    // Das Grid wird gesetzt, nicht erkannt. Gemessen wird hier die Schätzung
    // aus dem Klang gegen die Mitschrift; ein Grid, das um ein halbes BPM
    // danebenliegt, verschöbe nur, *wann* der Griff läuft — und die Mitschrift
    // schriebe es genauso mit. Es zu erkennen kostete Rechenzeit ohne Gewinn.
    for eintrag in pult.decks() {
        eintrag
            .state
            .set_grid(Some(Beatgrid::new(fall.bpm as f32, 0, 1.0)));
    }

    pult.aufnahme_setzen(aufnahme);
    Ok((pult, runner))
}

/// Kick, Bass und ein Akkord — genug für Grid und Harmonik.
///
/// Der Akkord ist das Entscheidende: Der Kritiker vergleicht Chroma, und ohne
/// Harmonik hätte er nichts zu vergleichen. Ein Klick-Track allein wäre für
/// dieses Verfahren unsichtbar, egal wie deutlich der Wechsel klingt.
fn material(bpm: f64, halbtoene: i32, secs: f64) -> Track {
    let n = (RATE as f64 * secs) as usize;
    let je_beat = RATE as f64 * 60.0 / bpm;
    let mut mono = vec![0.0f32; n];

    let grund = 110.0 * 2f32.powf(halbtoene as f32 / 12.0);
    // Ein Dur-Dreiklang: Grundton, große Terz, Quinte.
    for (i, s) in mono.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        for (halb, pegel) in [(0.0f32, 0.22f32), (4.0, 0.16), (7.0, 0.16)] {
            let f = grund * 2f32.powf(halb / 12.0) * 2.0;
            *s += pegel * (std::f32::consts::TAU * f * t).sin();
        }
    }

    let mut beat = 0usize;
    loop {
        let start = (beat as f64 * je_beat) as usize;
        let laenge = RATE as usize / 5;
        if start + laenge >= n {
            break;
        }
        for i in 0..laenge {
            let t = i as f32 / RATE as f32;
            let f = 120.0 * (-t * 28.0).exp() + 48.0;
            mono[start + i] += (std::f32::consts::TAU * f * t).sin() * (-t * 20.0).exp() * 0.8;
        }
        beat += 1;
    }

    Track {
        samples: mono.iter().flat_map(|v| [*v, *v]).collect(),
        sample_rate: RATE,
        stems: Vec::new(),
    }
}

/// Eine Zeile durch das Pult, wie sie über den Socket käme.
fn fuehren(pult: &mut Steuerpult, zeile: &str) -> Result<String> {
    let mut sitzung = Sitzung::neu();
    let antwort = behandle(pult, &mut sitzung, zeile);
    if antwort.starts_with("fehler") {
        bail!("{zeile} → {antwort}");
    }
    Ok(antwort)
}

/// Rendert und lässt dabei den Plan laufen — wie Soundkarte und Taktgeber.
///
/// **Mit Zügel.** Der Mitschnitt hängt an einem Ring von zwei Sekunden, und
/// wer schneller hineinschreibt, als der Schreiber herausholt, verliert die
/// Differenz. Hier wird deshalb nach jeweils einer halben Sekunde Musik kurz
/// angehalten. Das ist immer noch etwa fünfzigmal schneller als in Echtzeit,
/// und der Ring bleibt halb leer.
fn laufen(pult: &mut Steuerpult, runner: &mut EngineRunner, sekunden: f64) {
    let frames = (RATE as f64 * sekunden) as usize;
    let zuegel = RATE as usize / 4;
    let mut puffer = vec![0.0f32; BLOCK * 4];
    let mut getan = 0;
    let mut seit_pause = 0;
    while getan < frames {
        runner.render(&mut puffer, 4);
        getan += BLOCK;
        seit_pause += BLOCK;
        if seit_pause >= zuegel {
            seit_pause = 0;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Der Taktgeber läuft in der Anwendung alle 5 ms; hier alle 256 Frames,
        // also gut alle 5 ms bei 48 kHz. Näher am Ernstfall geht es nicht.
        let mut plan = std::mem::take(&mut pult.plan);
        takt(pult, &mut plan, &mut |p, zeile| {
            let mut s = Sitzung::neu();
            behandle(p, &mut s, zeile)
        });
        let neu = std::mem::take(&mut pult.plan);
        pult.plan = plan;
        for a in neu.auftraege() {
            pult.plan.uebernehmen(a.clone());
        }
    }
}

/// Wartet, bis der Schreiber-Thread die Datei **geschlossen** hat.
///
/// „Größer als der Kopf" reicht nicht: Die Länge im RIFF-Kopf wird erst beim
/// Schließen nachgetragen, und eine Datei mit Daten, aber falschem Kopf lässt
/// sich nicht dekodieren — beim ersten Lauf brach der Durchgang genau daran
/// ab. Geprüft wird deshalb der Kopf selbst.
fn warten_auf(wav: &Path) -> Result<()> {
    for _ in 0..400 {
        if kopf_stimmt(wav).unwrap_or(false) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    bail!("{} wurde nicht fertig geschrieben", wav.display())
}

/// Ob die Länge im RIFF-Kopf zur Datei passt.
fn kopf_stimmt(wav: &Path) -> std::io::Result<bool> {
    use std::io::Read;
    let laenge = std::fs::metadata(wav)?.len();
    if laenge < 44 {
        return Ok(false);
    }
    let mut kopf = [0u8; 8];
    std::fs::File::open(wav)?.read_exact(&mut kopf)?;
    let riff = u32::from_le_bytes([kopf[4], kopf[5], kopf[6], kopf[7]]) as u64;
    Ok(riff + 8 == laenge)
}

/// Die Zahl aus einer Antwort wie `value master.record_dropped 12`.
fn zahl(antwort: &str) -> f64 {
    antwort
        .split_whitespace()
        .next_back()
        .and_then(|w| w.parse().ok())
        .unwrap_or(0.0)
}

fn argumente() -> Result<PathBuf> {
    let mut args = std::env::args().skip(1);
    match args.next() {
        Some(a) if a == "--help" || a == "-h" => {
            println!("Aufruf: musik-streuung <ordner>");
            println!();
            println!("Fährt eine Reihe Sets durch die echte Anlage, legt Mitschnitt und");
            println!("Mitschrift in <ordner> ab und hält die Schätzung des Kritikers");
            println!("gegen das, was die Mitschrift festgehalten hat.");
            std::process::exit(0);
        }
        Some(a) => Ok(PathBuf::from(a)),
        None => bail!("Aufruf: musik-streuung <ordner>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protokoll(zeilen: &[&str]) -> mitschrift::Protokoll {
        let mut text = String::from("# musik-mitschrift 1\n# mitschnitt set.wav\n# rate 48000\n");
        text.push_str(&zeilen.join("\n"));
        mitschrift::aus_text(&text).expect("Mitschrift lesbar")
    }

    /// **Der Anker sitzt auf der Bewegung, nicht auf der Bestellung.**
    ///
    /// Beide falschen Anker sind einmal gemessen worden, und beide sahen nach
    /// Messwerten aus. Deshalb steht hier eine Mitschrift, die alle drei
    /// Zeilen enthält — Einrichten, Bestellung, Bewegung.
    #[test]
    fn die_wahrheit_ist_die_bewegung_und_nicht_die_bestellung() {
        let p = protokoll(&[
            "0 0.000 deck1=0.000/16~ > set master.crossfader -1",
            "0 0.000 deck1=0.000/16~ < ok master.crossfader -1",
            "1152000 24.000 deck1=51.200/16 > do master.uebergang blende 16",
            "1152000 24.000 deck1=51.200/16 > in deck1 phrase ramp master.crossfader 1 16 weich",
            "1152000 24.000 deck1=51.200/16 < ok plan 2 in 12.8 Beats: ramp master.crossfader 1 16",
            "1440000 30.000 deck1=64.000/16 > ramp master.crossfader 1 16 weich deck1",
            "1800192 37.504 deck1=80.009/16 < plan 4 fertig master.crossfader 1.0000",
        ]);
        assert_eq!(erste_bewegung(&p), Some(30.0));
    }

    /// Ohne Griff keine Wahrheit — und kein geratener Wert.
    #[test]
    fn ohne_uebergang_gibt_es_keine_wahrheit() {
        let p = protokoll(&[
            "0 0.000 deck1=0.000/16~ > set master.crossfader -1",
            "0 0.000 deck1=0.000/16~ < ok master.crossfader -1",
        ]);
        assert_eq!(erste_bewegung(&p), None);
    }

    /// Ein bestellter Griff, der nie ausgeführt wurde, ist keine Bewegung.
    #[test]
    fn eine_bestellung_allein_ist_keine_bewegung() {
        let p = protokoll(&[
            "1152000 24.000 deck1=51.200/16 > do master.uebergang blende 16",
            "1152000 24.000 deck1=51.200/16 > in deck1 phrase ramp master.crossfader 1 16 weich",
        ]);
        assert_eq!(erste_bewegung(&p), None);
    }

    #[test]
    fn die_zahl_kommt_aus_der_antwort() {
        assert_eq!(zahl("value master.record_dropped 1206680"), 1_206_680.0);
        assert_eq!(zahl("value master.record_seconds 18.86"), 18.86);
        assert_eq!(zahl("ok"), 0.0);
    }
}
