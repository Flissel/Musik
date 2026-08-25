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
    let mut block = vec![0.0f32; BLOCK * 2];
    let mut spitze = 0.0f32;
    let mut summe = 0.0f64;
    let mut gezaehlt = 0u64;
    let start = std::time::Instant::now();

    while start.elapsed().as_secs_f64() < opts.sekunden {
        source.render(&mut block);
        for s in &block {
            spitze = spitze.max(s.abs());
            summe += (*s as f64) * (*s as f64);
        }
        gezaehlt += block.len() as u64;
        // Nicht schneller lesen, als das Gerät liefert — sonst besteht die
        // Messung fast nur aus Unterläufen, die keine sind.
        std::thread::sleep(std::time::Duration::from_micros(
            (BLOCK as u64 * 1_000_000) / RATE as u64,
        ));
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
}

fn argumente() -> Result<Optionen> {
    let mut opts = Optionen {
        liste: false,
        geraet: None,
        sekunden: 5.0,
    };
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
                    .context("--sekunden braucht eine Zahl")?
            }
            "-h" | "--help" => {
                println!("Aufruf: musik-eingang [--liste] [--gerät <name>] [--sekunden <n>]");
                println!();
                println!("Öffnet ein Aufnahmegerät auf demselben Weg wie die Anwendung,");
                println!("hört zu und sagt, ob etwas ankommt, wie laut und ob lückenlos.");
                std::process::exit(0);
            }
            other => bail!("unbekannte Option: {other}"),
        }
    }
    Ok(opts)
}
