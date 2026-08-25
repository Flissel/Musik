//! Nachsehen, ob am AUX-Eingang wirklich etwas ankommt.
//!
//! Bevor jemand einen Abend lang mitschneidet, sollte er wissen, ob überhaupt
//! etwas hereinkommt und wie laut. Beides sieht man am Pult erst, wenn es zu
//! spät ist: Eine stille Stunde sieht aus wie eine Stunde, und eine
//! übersteuerte klingt erst beim Anhören falsch.
//!
//! Also vorher fragen. Das Werkzeug öffnet dasselbe Gerät auf demselben Weg
//! wie die Anwendung, hört ein paar Sekunden zu und sagt drei Dinge:
//!
//! - **Kommt etwas an?** Spitze und Mittelwert, in Dezibel.
//! - **Ist Luft nach oben?** Über −0,2 dBFS greift der Begrenzer der Summe ein,
//!   und was er einmal zusammengedrückt hat, ist im Mitschnitt zusammengedrückt.
//! - **Kommt es lückenlos an?** Unterläufe heißen: Der Ring lief leer, es fehlt
//!   Material. Überläufe heißen das Gegenteil.
//!
//! # Und mitschreiben
//!
//! `--aufnehmen <datei>` schreibt, was hereinkommt, direkt auf die Platte —
//! **am Mixer vorbei**. Das ist der Unterschied zu `do master.record`: Jener
//! nimmt die Summe auf, hinter Fader, EQ und Begrenzer, denn er ist für den
//! Mitschnitt eines Abends gedacht. Wer Lieder gewinnen will, will keinen
//! Abend, sondern die Quelle: ohne Begrenzer, ohne Kanalzug, so wie sie
//! ankommt.
//!
//! **Ein Gerät, ein Zugriff.** Die meisten Treiber lassen sich nur von einem
//! Programm zugleich aufnehmen. Entweder läuft `musik-app --aux-in` und der
//! Abend wird mit `do master.record` mitgeschnitten — oder `musik-eingang
//! --aufnehmen` holt die Quelle sauber ab. Beides gleichzeitig auf demselben
//! Gerät geht nicht, und was dann kommt, ist eine Fehlermeldung des Treibers.
//!
//! Der Kopf der Datei wird jede Sekunde nachgetragen. Eine WAV-Datei trägt ihre
//! Länge vorn, und die kennt man beim Anlegen noch nicht; wer die Aufnahme mit
//! Strg-C beendet, hätte sonst eine Datei mit falschem Kopf, die manche
//! Programme gar nicht lesen. So fehlt im schlimmsten Fall die letzte Sekunde.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use audio_engine::source::Source;
use audio_engine::{Eingang, aux_channel, eingang};

const RATE: u32 = 48_000;
const BLOCK: usize = 512;

/// Ab hier greift der Begrenzer der Summe ein.
///
/// Er steht bei 0,98 — das sind rund −0,18 dBFS. Wer darunter bleibt, kommt
/// unangetastet durch; wer darüber geht, bekommt eine Dynamik, die er nicht
/// gewählt hat.
const KOPFRAUM: f32 = 0.98;

fn main() -> Result<()> {
    let opts = argumente()?;

    if opts.liste {
        let geraete = eingang::geraete();
        if geraete.is_empty() {
            println!("Kein Aufnahmegerät gefunden.");
            return Ok(());
        }
        println!("Aufnahmegeräte:");
        for g in geraete {
            println!("  {g}");
        }
        return Ok(());
    }

    let (writer, mut source) = aux_channel(RATE as usize * 2);
    let eingang =
        Eingang::open(writer, RATE, opts.geraet.as_deref()).map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("Eingang: {}", eingang.device_name());
    println!(
        "  {} Kanal/Kanäle bei {} Hz, gehört wird {:.0} s\n",
        eingang.channels(),
        eingang.sample_rate(),
        opts.sekunden
    );

    // Denselben Weg nehmen wie der Mixer: aus der Quelle lesen, Block für
    // Block. Wer stattdessen den Ring direkt anschaute, prüfte etwas anderes
    // als das, was später läuft.
    let mut schreiber = match &opts.aufnehmen {
        Some(pfad) => {
            println!(
                "Mitgeschrieben wird nach {} — am Mixer vorbei.",
                pfad.display()
            );
            if opts.sekunden.is_infinite() {
                println!("Beenden mit Strg-C; der Kopf wird jede Sekunde nachgetragen.\n");
            } else {
                println!();
            }
            Some(
                WavSchreiber::anlegen(pfad, RATE)
                    .with_context(|| format!("{} ließ sich nicht anlegen", pfad.display()))?,
            )
        }
        None => None,
    };

    let mut block = vec![0.0f32; BLOCK * 2];
    let mut spitze = 0.0f32;
    let mut summe = 0.0f64;
    let mut gezaehlt = 0u64;
    let start = std::time::Instant::now();
    let mut zuletzt_nachgetragen = std::time::Instant::now();

    while start.elapsed().as_secs_f64() < opts.sekunden {
        source.render(&mut block);
        for s in &block {
            spitze = spitze.max(s.abs());
            summe += (*s as f64) * (*s as f64);
        }
        gezaehlt += block.len() as u64;

        if let Some(w) = schreiber.as_mut() {
            w.schreiben(&block).context("Schreiben fehlgeschlagen")?;
            if zuletzt_nachgetragen.elapsed().as_secs_f64() >= 1.0 {
                w.kopf_nachtragen()
                    .context("Kopf nachtragen fehlgeschlagen")?;
                zuletzt_nachgetragen = std::time::Instant::now();
            }
        }
        // Nicht schneller lesen, als das Gerät liefert — sonst besteht die
        // Messung fast nur aus Unterläufen, die keine sind.
        std::thread::sleep(std::time::Duration::from_micros(
            (BLOCK as u64 * 1_000_000) / RATE as u64,
        ));
    }

    if let Some(mut w) = schreiber.take() {
        let frames = w.frames();
        w.abschliessen().context("Abschließen fehlgeschlagen")?;
        println!(
            "Geschrieben: {:.1} s nach {}\n",
            frames as f64 / RATE as f64,
            opts.aufnehmen
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        );
    }

    let rms = (summe / gezaehlt.max(1) as f64).sqrt() as f32;
    println!("── Was ankam ────────────────────────────────");
    println!("  Spitze     {}", db(spitze));
    println!("  Mittelwert {}", db(rms));
    // Der erste Block ist immer leer: Gelesen wird, sobald das Gerät offen ist,
    // gefüllt wird es erst mit dem ersten Callback. Am `null`-Gerät gemessen —
    // genau 1024 Werte, also genau ein Block. Das als Fehler zu melden hieße,
    // bei jedem gesunden Eingang zu warnen.
    let echte = source.underruns().saturating_sub(block.len() as u64);
    let anteil = echte as f64 / gezaehlt.max(1) as f64;
    println!(
        "  Unterläufe {echte} von {gezaehlt} Werten ({:.2} %), Anlauf abgezogen",
        anteil * 100.0
    );

    println!();
    if spitze <= f32::EPSILON {
        println!("⚠ Es kam nichts an. Das Gerät ist offen, aber still — Kabel,");
        println!("  Eingangswahl am Interface oder die Quelle selbst.");
    } else if spitze > KOPFRAUM {
        println!(
            "⚠ Über {:.2} — der Begrenzer der Summe greift ein.",
            KOPFRAUM
        );
        println!("  Was er zusammendrückt, ist im Mitschnitt zusammengedrückt.");
        println!("  Am Gerät leiser machen, nicht in der Anwendung.");
    } else if spitze < 0.05 {
        println!("⚠ Sehr leise. Aufnehmen geht, aber die Analyse arbeitet dann");
        println!("  dicht am Rauschen — am Gerät lauter wäre besser.");
    } else {
        println!("Der Pegel taugt: unter dem Begrenzer und deutlich über dem Rauschen.");
    }

    if echte > 0 {
        println!();
        println!("⚠ {echte} Unterläufe nach dem Anlauf — es fehlt Material.");
        println!("  Zwei Uhren, die auseinanderlaufen. Ein größerer Ring hilft,");
        println!("  eine überlastete Maschine nicht.");
    }

    Ok(())
}

/// Eine WAV-Datei, die wächst und deren Kopf laufend nachgetragen wird.
///
/// Die Länge steht in einer WAV-Datei vorn, und beim Anlegen kennt man sie
/// nicht. Der übliche Weg ist, am Ende zurückzuspringen — nur endet eine
/// Aufnahme, die bis Strg-C läuft, nie ordentlich. Deshalb wird der Kopf jede
/// Sekunde nachgetragen: Im schlimmsten Fall fehlt die letzte Sekunde, statt
/// dass die ganze Datei unlesbar ist.
struct WavSchreiber {
    schreiber: BufWriter<File>,
    frames: u64,
}

impl WavSchreiber {
    fn anlegen(pfad: &std::path::Path, rate: u32) -> std::io::Result<WavSchreiber> {
        let mut schreiber = BufWriter::new(File::create(pfad)?);
        schreiber.write_all(b"RIFF")?;
        schreiber.write_all(&0u32.to_le_bytes())?;
        schreiber.write_all(b"WAVEfmt ")?;
        schreiber.write_all(&16u32.to_le_bytes())?;
        schreiber.write_all(&1u16.to_le_bytes())?;
        schreiber.write_all(&2u16.to_le_bytes())?;
        schreiber.write_all(&rate.to_le_bytes())?;
        schreiber.write_all(&(rate * 4).to_le_bytes())?;
        schreiber.write_all(&4u16.to_le_bytes())?;
        schreiber.write_all(&16u16.to_le_bytes())?;
        schreiber.write_all(b"data")?;
        schreiber.write_all(&0u32.to_le_bytes())?;
        Ok(WavSchreiber {
            schreiber,
            frames: 0,
        })
    }

    fn frames(&self) -> u64 {
        self.frames
    }

    fn schreiben(&mut self, block: &[f32]) -> std::io::Result<()> {
        for s in block {
            let wert = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
            self.schreiber.write_all(&wert.to_le_bytes())?;
        }
        self.frames += (block.len() / 2) as u64;
        Ok(())
    }

    /// Trägt die beiden Längen im Kopf nach und springt ans Ende zurück.
    fn kopf_nachtragen(&mut self) -> std::io::Result<()> {
        let daten = (self.frames * 4) as u32;
        self.schreiber.seek(SeekFrom::Start(4))?;
        self.schreiber.write_all(&(36 + daten).to_le_bytes())?;
        self.schreiber.seek(SeekFrom::Start(40))?;
        self.schreiber.write_all(&daten.to_le_bytes())?;
        self.schreiber.seek(SeekFrom::End(0))?;
        Ok(())
    }

    fn abschliessen(&mut self) -> std::io::Result<()> {
        self.kopf_nachtragen()?;
        self.schreiber.flush()
    }
}

/// Pegel in Dezibel, mit einem Boden statt eines minus Unendlich.
fn db(wert: f32) -> String {
    if wert <= 1e-6 {
        return "  still".to_string();
    }
    format!("{:+6.1} dBFS", 20.0 * wert.log10())
}

struct Optionen {
    liste: bool,
    geraet: Option<String>,
    sekunden: f64,
    /// Wohin mitgeschrieben wird. `None` heißt: nur hinhören.
    aufnehmen: Option<PathBuf>,
}

fn argumente() -> Result<Optionen> {
    let mut opts = Optionen {
        liste: false,
        geraet: None,
        // Zum Hinhören reichen fünf Sekunden. Beim Aufnehmen wäre das eine
        // Falle, deshalb steht die Vorgabe dort woanders.
        sekunden: 5.0,
        aufnehmen: None,
    };
    let mut sekunden_gesetzt = false;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--liste" => opts.liste = true,
            "--gerät" | "--geraet" => {
                opts.geraet = Some(iter.next().context("--gerät braucht einen Namen")?)
            }
            "--sekunden" => {
                opts.sekunden = iter
                    .next()
                    .context("--sekunden braucht eine Zahl")?
                    .parse()
                    .context("--sekunden braucht eine Zahl")?;
                sekunden_gesetzt = true;
            }
            "--aufnehmen" => {
                opts.aufnehmen = Some(PathBuf::from(
                    iter.next().context("--aufnehmen braucht einen Pfad")?,
                ))
            }
            "-h" | "--help" => {
                println!("Aufruf: musik-eingang [--liste] [--gerät <name>] [--sekunden <n>]");
                println!("                      [--aufnehmen <datei.wav>]");
                println!();
                println!("Öffnet ein Aufnahmegerät auf demselben Weg wie die Anwendung,");
                println!("hört zu und sagt, ob etwas ankommt, wie laut und ob lückenlos.");
                println!();
                println!("--aufnehmen schreibt mit — am Mixer vorbei, also ohne Begrenzer");
                println!("            und ohne Kanalzug. Ohne --sekunden läuft es, bis");
                println!("            Strg-C kommt. Danach: musik-schneiden <datei> <ordner>");
                println!();
                println!("Ein Gerät lässt sich meist nur von einem Programm zugleich");
                println!("aufnehmen: entweder musik-app --aux-in oder dieses hier.");
                std::process::exit(0);
            }
            other => bail!("unbekannte Option: {other}"),
        }
    }
    // Wer aufnimmt und nichts sagt, meint „bis ich aufhöre" — nicht fünf
    // Sekunden. Fünf Sekunden wären genau die Art Vorgabe, die einem erst nach
    // dem Abend auffällt.
    if opts.aufnehmen.is_some() && !sekunden_gesetzt {
        opts.sekunden = f64::INFINITY;
    }
    Ok(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("musik-eingang-test-{name}.wav"))
    }

    /// **Eine abgebrochene Aufnahme bleibt lesbar.**
    ///
    /// Der Kopf trägt die Länge, und die kennt man beim Anlegen nicht. Wer erst
    /// am Ende zurückspringt, hat nach einem Strg-C eine Datei, die manche
    /// Programme gar nicht öffnen. Hier steht, dass der nachgetragene Kopf zu
    /// dem passt, was tatsächlich geschrieben wurde — auch mitten im Lauf.
    #[test]
    fn der_kopf_passt_auch_mitten_in_der_aufnahme() {
        let pfad = temp("mitten");
        let mut w = WavSchreiber::anlegen(&pfad, 48_000).expect("anlegen");
        for _ in 0..10 {
            w.schreiben(&[0.25f32; 512]).expect("schreiben");
        }
        w.kopf_nachtragen().expect("nachtragen");
        // Ohne Abschließen weiterschreiben: genau der Zustand, in dem ein
        // Strg-C die Datei erwischt.
        w.schreiben(&[0.25f32; 512]).expect("schreiben");
        drop(w);

        let (riff, daten) = kopf_lesen(&pfad);
        assert_eq!(daten, 10 * 512 * 2, "die Datenlänge stimmt nicht");
        assert_eq!(
            riff,
            36 + daten,
            "die RIFF-Länge passt nicht zur Datenlänge"
        );
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn nach_dem_abschliessen_stimmt_alles() {
        let pfad = temp("fertig");
        let mut w = WavSchreiber::anlegen(&pfad, 48_000).expect("anlegen");
        w.schreiben(&[0.5f32; 2_048]).expect("schreiben");
        assert_eq!(w.frames(), 1_024);
        w.abschliessen().expect("abschließen");
        drop(w);

        let laenge = std::fs::metadata(&pfad).expect("metadata").len();
        let (riff, daten) = kopf_lesen(&pfad);
        assert_eq!(daten, 2_048 * 2);
        assert_eq!(riff as u64 + 8, laenge, "der Kopf passt nicht zur Datei");
        let _ = std::fs::remove_file(&pfad);
    }

    fn kopf_lesen(pfad: &std::path::Path) -> (u32, u32) {
        let roh = std::fs::read(pfad).expect("lesen");
        let riff = u32::from_le_bytes([roh[4], roh[5], roh[6], roh[7]]);
        let daten = u32::from_le_bytes([roh[40], roh[41], roh[42], roh[43]]);
        (riff, daten)
    }
}
