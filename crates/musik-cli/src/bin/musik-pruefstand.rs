//! Der Prüfstand: legt die Analyse neben das, was ein Mensch hört.
//!
//! **Warum es das braucht.** Über der Anlage liegen drei Schichten Messwerkzeug
//! — der Kritiker, die Mitschrift und die Gliederung. Jede hat beim Bauen
//! sofort einen Fehler gefunden, den niemand vermutet hatte. Und jede Schwelle
//! darin ist an Material geeicht, das aus eigener Hand stammt.
//!
//! Genau das war in diesem Projekt schon viermal die Vorstufe eines Fehlers:
//! Die Güteschwelle beim Tempo war an Klick-Tracks geeicht, die zwischen den
//! Klicks still sind; bei der Tonart wies eine synthetisch geeichte Schwelle
//! vier von fünf echten Aufnahmen ab; und bei der Gliederung war der Prüfstein
//! „gleichförmiges Material" ein reiner Sinus, dessen spektraler Fluss um 45 %
//! driftet. Jedes Mal sah es vorher gut aus.
//!
//! Ein Messwerkzeug, dem niemand widerspricht, ist ein Orakel. Hier
//! widerspricht ihm ein Mensch.
//!
//! # Die Wahrheitsdatei
//!
//! Eine Zeile je Track, von Hand geschrieben, Zeiten als `m:ss` oder in
//! Sekunden:
//!
//! ```text
//! # was ich höre
//! Nachtschicht.mp3  bpm 124  tonart Am  intro 0:00  aufbau 0:32  drop 1:04  outro 5:12
//! Alpenglühen.wav   bpm 126
//! ```
//!
//! Abschnitte stehen mit ihrem **Anfang**, nicht als Bereich: Wer zuhört,
//! notiert „hier fängt das Outro an" und nicht „das Outro geht von … bis …".
//! Das Ende ist der Anfang des nächsten. Alle Angaben sind einzeln freiwillig —
//! wer nur das Tempo weiß, schreibt nur das Tempo.
//!
//! # Was er meldet und was nicht
//!
//! **Abweichungen, keine Fehler.** Eine Angabe von Hand ist eine Meinung: Zwei
//! Leute setzen den Anfang eines Drops eine Phrase auseinander, und beide haben
//! recht. Ab welchem Abstand etwas kaputt ist, entscheidet weiterhin, wer
//! zuhört — deshalb steht hier kein Urteil und keine Note, sondern eine Zahl
//! mit Vorzeichen.
//!
//! Aus demselben Grund endet er mit Rückgabewert 0, auch wenn alles daneben
//! liegt. Was nicht gelesen werden konnte, ist ein Fehler; was nicht
//! übereinstimmt, ist ein Befund.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use analysis::{Store, analyze_cached};
use audio_core::{Art, Tonart, Track};

const DEFAULT_CACHE: &str = ".musik-analyse";

/// Ab welchem Abstand zwei Tempi als verschieden gelten.
///
/// Ein halbes BPM ist unter dem, was ein Mensch beim Zählen trifft, und über
/// dem, was der Detektor zwischen zwei Läufen schwankt.
const BPM_GENAU: f32 = 0.5;

/// Wie weit eine erkannte Grenze von einer gehörten entfernt sein darf, um noch
/// dieselbe zu sein — in Phrasen.
///
/// Eine ganze Phrase, weil die Angabe von Hand genau so grob ist: Wer den
/// Anfang eines Drops notiert, trifft die Phrase und selten den Beat.
const ZUORDNUNG_PHRASEN: f64 = 1.0;

fn main() -> Result<()> {
    let opts = argumente()?;
    let wahrheiten = wahrheit_lesen(&opts.wahrheit)?;
    if wahrheiten.is_empty() {
        bail!("{} enthält keine Angaben", opts.wahrheit.display());
    }

    let store = Store::new(&opts.cache);
    println!("Wahrheit: {}", opts.wahrheit.display());
    println!("Cache:    {}", store.root().display());
    println!(
        "\n{} Tracks. Angegeben ist, was ein Mensch hört; gemeldet wird der",
        wahrheiten.len()
    );
    println!("Abstand dazu — mit Vorzeichen, ohne Urteil.\n");

    let mut summe = Summe::default();
    for w in &wahrheiten {
        match ein_track(w, &store, &mut summe) {
            Ok(()) => {}
            Err(e) => {
                println!("── {} ──\n  nicht lesbar: {e:#}\n", w.datei.display());
                summe.unlesbar += 1;
            }
        }
    }

    summe.bericht();
    Ok(())
}

// ── Die Wahrheitsdatei ───────────────────────────────────────────────────

/// Was ein Mensch über einen Track gesagt hat.
#[derive(Debug, Clone, PartialEq)]
struct Wahrheit {
    datei: PathBuf,
    bpm: Option<f32>,
    tonart: Option<Tonart>,
    /// Abschnitte mit ihrem Anfang in Sekunden, in der Reihenfolge der Zeile.
    abschnitte: Vec<(Art, f64)>,
}

/// Liest die Wahrheitsdatei.
///
/// Pfade sind relativ zu ihr selbst zu verstehen — sie liegt neben der Musik,
/// und dann steht in ihr der bloße Dateiname.
fn wahrheit_lesen(pfad: &Path) -> Result<Vec<Wahrheit>> {
    let text = std::fs::read_to_string(pfad)
        .with_context(|| format!("{} ließ sich nicht lesen", pfad.display()))?;
    let ordner = pfad.parent().unwrap_or(Path::new("."));

    let mut aus = Vec::new();
    for (nummer, zeile) in text.lines().enumerate() {
        let Some(mut w) = zeile_lesen(zeile)
            .with_context(|| format!("{}, Zeile {}", pfad.display(), nummer + 1))?
        else {
            continue;
        };
        if w.datei.is_relative() {
            w.datei = ordner.join(&w.datei);
        }
        aus.push(w);
    }
    Ok(aus)
}

/// Eine Zeile. `None` für Leerzeilen und Kommentare.
fn zeile_lesen(zeile: &str) -> Result<Option<Wahrheit>> {
    let zeile = zeile.trim();
    if zeile.is_empty() || zeile.starts_with('#') {
        return Ok(None);
    }

    let mut worte = zeile.split_whitespace();
    let datei = worte.next().expect("nicht leer");
    let mut w = Wahrheit {
        datei: PathBuf::from(datei),
        bpm: None,
        tonart: None,
        abschnitte: Vec::new(),
    };

    while let Some(schluessel) = worte.next() {
        let Some(wert) = worte.next() else {
            bail!("{schluessel} steht ohne Wert da");
        };
        match schluessel {
            "bpm" => {
                w.bpm = Some(
                    wert.parse()
                        .with_context(|| format!("{wert} ist kein Tempo"))?,
                )
            }
            "tonart" => {
                w.tonart = Some(
                    Tonart::parse(wert)
                        .with_context(|| format!("{wert} ist keine Tonart (Am, F#, 8A)"))?,
                )
            }
            andere => {
                let art = Art::parse(andere).with_context(|| {
                    format!("{andere} ist weder bpm, tonart noch ein Abschnitt")
                })?;
                w.abschnitte.push((art, zeit_lesen(wert)?));
            }
        }
    }

    // Sortiert, weil alles Weitere davon ausgeht — und weil eine Zeile, in der
    // die Zeiten durcheinandergeraten sind, sonst still falsch verglichen
    // würde.
    w.abschnitte
        .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Some(w))
}

/// `m:ss`, `m:ss.s` oder eine Sekundenzahl.
fn zeit_lesen(text: &str) -> Result<f64> {
    match text.split_once(':') {
        Some((min, sek)) => {
            let min: f64 = min
                .parse()
                .with_context(|| format!("{text} ist keine Zeit"))?;
            let sek: f64 = sek
                .parse()
                .with_context(|| format!("{text} ist keine Zeit"))?;
            Ok(min * 60.0 + sek)
        }
        None => text
            .parse()
            .with_context(|| format!("{text} ist keine Zeit")),
    }
}

// ── Der Vergleich ────────────────────────────────────────────────────────

/// Wie eine erkannte Tonart zur gehörten steht.
///
/// Nicht bloß richtig oder falsch: Eine Paralleltonart zu treffen ist ein
/// anderer Fehler, als danebenzugreifen — beim Mischen tut sie nicht weh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tonartlage {
    Gleich,
    /// Auf dem Camelot-Rad benachbart oder parallel — mischbar.
    Verwandt,
    Daneben,
}

fn tonartlage(erkannt: Tonart, gehoert: Tonart) -> Tonartlage {
    if erkannt == gehoert {
        Tonartlage::Gleich
    } else if erkannt.passt_zu(&gehoert) {
        Tonartlage::Verwandt
    } else {
        Tonartlage::Daneben
    }
}

/// Wie das Tempo zum gehörten steht.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tempolage {
    Genau,
    /// Doppelt oder halb so schnell — der klassische Oktavfehler.
    Oktave,
    Daneben,
}

fn tempolage(erkannt: f32, gehoert: f32) -> Tempolage {
    if (erkannt - gehoert).abs() <= BPM_GENAU {
        return Tempolage::Genau;
    }
    for faktor in [0.5f32, 2.0, 0.25, 4.0, 1.0 / 3.0, 3.0] {
        if (erkannt - gehoert * faktor).abs() <= BPM_GENAU {
            return Tempolage::Oktave;
        }
    }
    Tempolage::Daneben
}

#[derive(Default)]
struct Summe {
    tracks: usize,
    unlesbar: usize,
    tempo: BTreeMap<&'static str, usize>,
    tonart: BTreeMap<&'static str, usize>,
    /// Abstände erkannter zu gehörten Grenzen, in Beats.
    grenzen: Vec<f64>,
    /// Wie oft der Name am Abschnittsmittelpunkt übereinstimmte.
    namen_treffer: usize,
    namen_gesamt: usize,
    /// Erkannte Grenzen ohne gehörtes Gegenstück.
    ueberzaehlig: usize,
    /// Gehörte Grenzen ohne erkanntes Gegenstück.
    fehlend: usize,
}

impl Summe {
    fn bericht(&self) {
        println!("══ Zusammen ══════════════════════════════════");
        println!("  {} Tracks gelesen", self.tracks);
        if self.unlesbar > 0 {
            println!("  ⚠ {} nicht lesbar", self.unlesbar);
        }

        if !self.tempo.is_empty() {
            let zeile: Vec<String> = self
                .tempo
                .iter()
                .map(|(k, v)| format!("{v}× {k}"))
                .collect();
            println!("  Tempo:  {}", zeile.join(", "));
        }
        if !self.tonart.is_empty() {
            let zeile: Vec<String> = self
                .tonart
                .iter()
                .map(|(k, v)| format!("{v}× {k}"))
                .collect();
            println!("  Tonart: {}", zeile.join(", "));
        }

        if !self.grenzen.is_empty() {
            let mut s: Vec<f64> = self.grenzen.iter().map(|d| d.abs()).collect();
            s.sort_by(f64::total_cmp);
            let median = s[s.len() / 2];
            let groesste = s.last().copied().unwrap_or(0.0);
            println!(
                "  Grenzen: {} zugeordnet, Median {median:.1} Beats daneben, größter Abstand {groesste:.1}",
                s.len()
            );
        }
        if self.fehlend > 0 || self.ueberzaehlig > 0 {
            println!(
                "  {} gehörte Grenzen ohne Fund, {} gefundene ohne Gegenstück",
                self.fehlend, self.ueberzaehlig
            );
        }
        if self.namen_gesamt > 0 {
            println!(
                "  Namen: {} von {} Abschnitten stimmen überein",
                self.namen_treffer, self.namen_gesamt
            );
        }

        println!();
        println!("  Das sind Abweichungen, keine Fehler. Eine Angabe von Hand ist");
        println!("  eine Meinung — zwei Leute setzen den Anfang eines Drops eine");
        println!("  Phrase auseinander, und beide haben recht.");
    }
}

/// Der gemeinsame Versatz aller Grenzen, falls es einen gibt.
///
/// **Ein konstanter Abstand ist eine Tatsache, keine sechs.** Liegen alle
/// Grenzen um dasselbe daneben, sagt das etwas über den Nullpunkt — den Anker
/// des Beatgrids gegen den Dateianfang — und nichts über die Segmentierung.
/// Streuen sie, ist es umgekehrt. Beides in einer Spalte übereinander zu
/// stapeln, verdeckt genau den Unterschied, auf den es ankommt.
///
/// Gibt `(Versatz, größter Rest danach)` zurück, oder `None`, wenn kein
/// nennenswerter gemeinsamer Anteil da ist.
fn versatz(abweichungen: &[f64]) -> Option<(f64, f64)> {
    if abweichungen.len() < 3 {
        return None;
    }
    let mut s = abweichungen.to_vec();
    s.sort_by(f64::total_cmp);
    let median = s[s.len() / 2];
    if median.abs() < 0.25 {
        return None;
    }
    let rest = s.iter().map(|d| (d - median).abs()).fold(0.0f64, f64::max);
    // Streuen sie stärker, als sie gemeinsam verschoben sind, ist es kein
    // Versatz, sondern Rauschen.
    (rest < median.abs()).then_some((median, rest))
}

fn ein_track(w: &Wahrheit, store: &Store, summe: &mut Summe) -> Result<()> {
    let track = Track::decode_file(&w.datei)
        .with_context(|| format!("konnte {} nicht dekodieren", w.datei.display()))?;
    let (analyse, gerechnet) = analyze_cached(&track, store);
    if gerechnet {
        store.save(&analyse).ok();
    }
    summe.tracks += 1;

    println!("── {} ──", w.datei.display());

    // ── Tempo ──
    if let Some(gehoert) = w.bpm {
        match analyse.bpm {
            Some(erkannt) => {
                let lage = tempolage(erkannt, gehoert);
                let name = match lage {
                    Tempolage::Genau => "genau",
                    Tempolage::Oktave => "Oktavfehler",
                    Tempolage::Daneben => "daneben",
                };
                *summe.tempo.entry(name).or_default() += 1;
                println!(
                    "  Tempo   {erkannt:7.2} gegen {gehoert:7.2} gehört   {:+.2}   {}",
                    erkannt - gehoert,
                    match lage {
                        Tempolage::Genau => "✓",
                        Tempolage::Oktave => "⚠ Oktavfehler",
                        Tempolage::Daneben => "⚠",
                    }
                );
            }
            None => {
                *summe.tempo.entry("nicht erkannt").or_default() += 1;
                println!("  Tempo   nicht erkannt, gehört {gehoert:.2}");
            }
        }
    }

    // ── Tonart ──
    if let Some(gehoert) = w.tonart {
        match analyse.tonart() {
            Some(erkannt) => {
                let lage = tonartlage(erkannt, gehoert);
                let name = match lage {
                    Tonartlage::Gleich => "gleich",
                    Tonartlage::Verwandt => "verwandt",
                    Tonartlage::Daneben => "daneben",
                };
                *summe.tonart.entry(name).or_default() += 1;
                println!(
                    "  Tonart  {:<4} ({}) gegen {} ({}) gehört   {}",
                    erkannt.name(),
                    erkannt.camelot(),
                    gehoert.name(),
                    gehoert.camelot(),
                    match lage {
                        Tonartlage::Gleich => "✓",
                        Tonartlage::Verwandt => "~ verwandt, mischbar",
                        Tonartlage::Daneben => "⚠",
                    }
                );
            }
            None => {
                *summe.tonart.entry("nicht erkannt").or_default() += 1;
                println!("  Tonart  nicht erkannt, gehört {}", gehoert.name());
            }
        }
    }

    if !w.abschnitte.is_empty() {
        abschnitte_vergleichen(w, &analyse, summe);
    }
    println!();
    Ok(())
}

fn abschnitte_vergleichen(w: &Wahrheit, analyse: &analysis::Analysis, summe: &mut Summe) {
    let Some(s) = analyse.struktur() else {
        println!(
            "  Gliederung  keine erkannt, gehört wurden {} Abschnitte",
            w.abschnitte.len()
        );
        summe.fehlend += w.abschnitte.len();
        summe.namen_gesamt += w.abschnitte.len();
        return;
    };

    let rate = analyse.sample_rate as f64;
    let je_beat = 60.0 / analyse.bpm.unwrap_or(120.0) as f64;
    let toleranz = ZUORDNUNG_PHRASEN * audio_core::PHRASE_BEATS * je_beat;
    let ende = analyse.duration_secs;

    println!(
        "  Gliederung  {} erkannt gegen {} gehört",
        s.abschnitte.len(),
        w.abschnitte.len()
    );

    let mut getroffen = vec![false; s.abschnitte.len()];
    let mut abweichungen: Vec<f64> = Vec::new();
    for (i, (art, beginn)) in w.abschnitte.iter().enumerate() {
        // Der Name wird in der **Mitte** verglichen, nicht an der Grenze: Dort
        // steht man mit einem Fuß im nächsten Abschnitt, und ein Vergleich auf
        // der Kante misst die Kante statt den Abschnitt.
        let bis = w
            .abschnitte
            .get(i + 1)
            .map(|(_, t)| *t)
            .unwrap_or(ende)
            .max(*beginn);
        let mitte = (beginn + bis) / 2.0;
        let erkannt_hier = s.bei_frames((mitte * rate) as u64).map(|a| a.art);

        summe.namen_gesamt += 1;
        let name_passt = erkannt_hier == Some(*art);
        if name_passt {
            summe.namen_treffer += 1;
        }

        // Nächste erkannte Grenze zu diesem Anfang.
        let naechste = s
            .abschnitte
            .iter()
            .enumerate()
            .map(|(k, a)| (k, a.von_frames as f64 / rate - beginn))
            .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));

        match naechste.filter(|(_, ab)| ab.abs() <= toleranz) {
            Some((k, ab)) => {
                getroffen[k] = true;
                summe.grenzen.push(ab / je_beat);
                abweichungen.push(ab / je_beat);
                println!(
                    "    {:>7.1}s  {:<6}  Grenze {:+.1}s ({:+.1} Beats)   Name {}",
                    beginn,
                    art.name(),
                    ab,
                    ab / je_beat,
                    if name_passt {
                        "✓".to_string()
                    } else {
                        format!(
                            "⚠ erkannt als {}",
                            erkannt_hier.map(|a| a.name()).unwrap_or("nichts")
                        )
                    }
                );
            }
            None => {
                summe.fehlend += 1;
                println!(
                    "    {:>7.1}s  {:<6}  ⚠ keine Grenze in der Nähe (±{:.0}s)",
                    beginn,
                    art.name(),
                    toleranz
                );
            }
        }
    }

    if let Some((versatz, rest)) = versatz(&abweichungen) {
        println!("    ── Davon systematisch: {versatz:+.1} Beats auf allen Grenzen ──");
        println!("    Das ist der Nullpunkt des Beatgrids, nicht die Segmentierung: Der");
        println!("    Anker liegt auf dem ersten erkannten Schlag, die Wahrheitsdatei");
        println!("    zählt ab Dateianfang. Relativ zueinander liegen die Grenzen um");
        println!("    höchstens {rest:.1} Beats daneben.");
    }

    // Was gefunden wurde, ohne dass jemand dort etwas gehört hat. Der erste
    // Abschnitt zählt nicht mit: Sein Anfang ist der Einstiegspunkt und keine
    // Grenze zwischen zwei Teilen.
    let ueber = getroffen.iter().skip(1).filter(|t| !**t).count();
    if ueber > 0 {
        summe.ueberzaehlig += ueber;
        println!("    {ueber} erkannte Grenzen ohne gehörtes Gegenstück");
    }
}

// ── Aufruf ───────────────────────────────────────────────────────────────

struct Optionen {
    wahrheit: PathBuf,
    cache: PathBuf,
}

fn argumente() -> Result<Optionen> {
    let mut wahrheit = None;
    let mut cache = PathBuf::from(DEFAULT_CACHE);

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                hilfe();
                std::process::exit(0);
            }
            "--cache" => {
                cache = PathBuf::from(args.next().context("--cache braucht ein Verzeichnis")?)
            }
            "--wahrheit" => {
                wahrheit = Some(PathBuf::from(
                    args.next().context("--wahrheit braucht eine Datei")?,
                ))
            }
            andere => wahrheit = Some(PathBuf::from(andere)),
        }
    }

    let Some(wahrheit) = wahrheit else {
        hilfe();
        bail!("keine Wahrheitsdatei angegeben");
    };
    if !wahrheit.exists() {
        bail!("{} gibt es nicht", wahrheit.display());
    }
    Ok(Optionen { wahrheit, cache })
}

fn hilfe() {
    println!("Aufruf: musik-pruefstand [--wahrheit] <datei> [--cache <dir>]");
    println!();
    println!("Legt die Analyse neben das, was ein Mensch gehört hat, und meldet den");
    println!("Abstand. Die Wahrheitsdatei liegt neben der Musik, eine Zeile je Track:");
    println!();
    println!("  # was ich höre");
    println!("  Nachtschicht.mp3  bpm 124  tonart Am  intro 0:00  drop 1:04  outro 5:12");
    println!("  Alpenglühen.wav   bpm 126");
    println!();
    println!("Abschnitte stehen mit ihrem Anfang, nicht als Bereich — das Ende ist der");
    println!("Anfang des nächsten. Jede Angabe ist einzeln freiwillig.");
    println!();
    println!("Gemeldet werden Abweichungen, keine Fehler: Eine Angabe von Hand ist eine");
    println!("Meinung. Ab wann etwas kaputt ist, entscheidet, wer zuhört.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eine_volle_zeile_wird_gelesen() {
        let w =
            zeile_lesen("Nachtschicht.mp3  bpm 124  tonart Am  intro 0:00  drop 1:04  outro 5:12")
                .expect("lesbar")
                .expect("keine Leerzeile");
        assert_eq!(w.datei, PathBuf::from("Nachtschicht.mp3"));
        assert_eq!(w.bpm, Some(124.0));
        assert_eq!(w.tonart, Tonart::parse("Am"));
        assert_eq!(
            w.abschnitte,
            vec![(Art::Intro, 0.0), (Art::Drop, 64.0), (Art::Outro, 312.0)]
        );
    }

    /// Wer nur das Tempo weiß, schreibt nur das Tempo.
    #[test]
    fn jede_angabe_ist_einzeln_freiwillig() {
        let w = zeile_lesen("a.wav bpm 126").unwrap().unwrap();
        assert_eq!(w.bpm, Some(126.0));
        assert!(w.tonart.is_none());
        assert!(w.abschnitte.is_empty());

        let w = zeile_lesen("b.wav outro 2:00").unwrap().unwrap();
        assert!(w.bpm.is_none());
        assert_eq!(w.abschnitte, vec![(Art::Outro, 120.0)]);
    }

    #[test]
    fn kommentare_und_leerzeilen_zaehlen_nicht() {
        assert_eq!(zeile_lesen("# was ich höre").unwrap(), None);
        assert_eq!(zeile_lesen("   ").unwrap(), None);
    }

    /// Zeiten in beiden Schreibweisen — und eine kaputte wird gemeldet, statt
    /// still zu 0 zu werden.
    #[test]
    fn zeiten_gehen_als_minuten_und_als_sekunden() {
        assert_eq!(zeit_lesen("1:04").unwrap(), 64.0);
        assert_eq!(zeit_lesen("0:07.5").unwrap(), 7.5);
        assert_eq!(zeit_lesen("64").unwrap(), 64.0);
        assert_eq!(zeit_lesen("12:00").unwrap(), 720.0);
        assert!(zeit_lesen("gleich").is_err());
    }

    /// Ein Tippfehler darf nicht als gültige Angabe durchgehen.
    #[test]
    fn unsinn_wird_gemeldet_statt_verschluckt() {
        assert!(zeile_lesen("a.wav bpm").is_err(), "Wert fehlt");
        assert!(zeile_lesen("a.wav bpm schnell").is_err());
        assert!(zeile_lesen("a.wav tonart H7").is_err());
        assert!(zeile_lesen("a.wav refrain 1:00").is_err(), "kein Abschnitt");
    }

    /// Durcheinandergeratene Zeiten würden sonst still falsch verglichen.
    #[test]
    fn abschnitte_werden_sortiert() {
        let w = zeile_lesen("a.wav outro 5:00 intro 0:00 drop 2:00")
            .unwrap()
            .unwrap();
        assert_eq!(
            w.abschnitte,
            vec![(Art::Intro, 0.0), (Art::Drop, 120.0), (Art::Outro, 300.0)]
        );
    }

    /// Der Oktavfehler ist ein eigener Befund, keine große Abweichung: 87 statt
    /// 174 ist etwas anderes als 87 statt 92.
    #[test]
    fn ein_oktavfehler_heisst_oktavfehler() {
        assert_eq!(tempolage(128.0, 128.0), Tempolage::Genau);
        assert_eq!(tempolage(128.2, 128.0), Tempolage::Genau);
        assert_eq!(tempolage(64.0, 128.0), Tempolage::Oktave);
        assert_eq!(tempolage(256.0, 128.0), Tempolage::Oktave);
        assert_eq!(tempolage(85.0, 128.0), Tempolage::Daneben);
    }

    /// Eine Paralleltonart zu treffen tut beim Mischen nicht weh. Das ist ein
    /// anderer Befund als danebenzugreifen.
    #[test]
    fn eine_verwandte_tonart_ist_nicht_dasselbe_wie_daneben() {
        let am = Tonart::parse("Am").unwrap();
        let c = Tonart::parse("C").unwrap();
        let fis = Tonart::parse("F#").unwrap();
        assert_eq!(tonartlage(am, am), Tonartlage::Gleich);
        assert_eq!(tonartlage(c, am), Tonartlage::Verwandt);
        assert_eq!(tonartlage(fis, am), Tonartlage::Daneben);
    }

    /// Alle Grenzen um dasselbe daneben heißt: Der Nullpunkt sitzt anders.
    /// Das ist ein Befund über den Anker und keiner über die Segmentierung.
    #[test]
    fn ein_gemeinsamer_versatz_wird_als_solcher_erkannt() {
        let (v, rest) = versatz(&[1.0, 1.0, 1.0, 1.05, 0.95]).expect("kein Versatz gefunden");
        assert!((v - 1.0).abs() < 1e-9, "{v}");
        assert!(rest <= 0.05 + 1e-9, "{rest}");
    }

    /// Streuung ist kein Versatz. Sonst würde aus „die Segmentierung wackelt"
    /// ein beruhigendes „nur der Nullpunkt".
    #[test]
    fn streuung_wird_nicht_zum_versatz_erklaert() {
        assert_eq!(versatz(&[2.0, -2.0, 1.0, -1.5, 0.5]), None);
        assert_eq!(
            versatz(&[0.1, 0.0, -0.1]),
            None,
            "zu klein für eine Aussage"
        );
        assert_eq!(versatz(&[1.0, 1.0]), None, "zwei sind kein Muster");
    }

    #[test]
    fn pfade_gelten_relativ_zur_wahrheitsdatei() {
        let ordner = std::env::temp_dir().join(format!("pruefstand-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Ordner");
        let pfad = ordner.join("wahrheit.txt");
        std::fs::write(&pfad, "# Kopf\nlied.wav bpm 120\n").expect("schreiben");

        let w = wahrheit_lesen(&pfad).expect("lesbar");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].datei, ordner.join("lied.wav"));

        std::fs::remove_dir_all(&ordner).ok();
    }
}
