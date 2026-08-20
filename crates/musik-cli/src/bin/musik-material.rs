//! Baut Prüfmaterial mit bekannter Gliederung — die schwierigen Fälle.
//!
//! Die Gliederung trägt inzwischen an drei Stellen: am Bogen (`arc_actual`),
//! an der Auswahl nach Energie (`master.search_next`) und an `beats_to_outro`,
//! nach dem ein Übergang liegt. Ihre Regeln haben unterwegs schon fünfmal am
//! Material gelegen — zuletzt bei einem Stück, das die meiste Zeit oben war
//! und deshalb „Intro, Break, Outro" hieß.
//!
//! Was dieses Werkzeug liefert, ist deshalb **kein Beweis, dass die Gliederung
//! stimmt.** Es liefert Fälle, bei denen von vornherein feststeht, was
//! herauskommen soll, und legt die Wahrheitsdatei gleich daneben:
//!
//! ```text
//! musik-material /tmp/schwierig
//! musik-pruefstand /tmp/schwierig/wahrheit.txt --cache /tmp/schwierig/.cache
//! ```
//!
//! # Warum gebautes Material und nicht Musik
//!
//! Weil hier die *Struktur* geprüft wird und nicht das Hören. Ein Track ohne
//! Outro ist als gebautes Stück eindeutig — bei echter Musik wäre schon
//! strittig, ob die letzten sechzehn Takte eines sind. Das eine ersetzt das
//! andere nicht: Echte Musik mit gehörter Wahrheit steht weiter aus und ist
//! der eigentliche Prüfstein (siehe `docs/FAHRPLAN.md`, M1).
//!
//! Gebaut wird mit denselben drei Zutaten wie das Material der Testsuite —
//! Kick, Bass, Akkord —, damit hier die Struktur den Unterschied macht und
//! nicht der Klang.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const RATE: u32 = 44_100;
const PHRASE_BEATS: f64 = 16.0;

/// Ein Abschnitt: wie er heißen soll, wie lang er ist, und woraus er besteht.
#[derive(Clone, Copy)]
struct Teil {
    art: &'static str,
    phrasen: usize,
    kick: bool,
    bass: bool,
    akkord: f32,
}

const fn teil(art: &'static str, phrasen: usize, kick: bool, bass: bool, akkord: f32) -> Teil {
    Teil {
        art,
        phrasen,
        kick,
        bass,
        akkord,
    }
}

/// Ein Fall: Dateiname, Tempo, Abschnitte — und wonach gefragt wird.
struct Fall {
    datei: &'static str,
    bpm: f64,
    /// Ab welchem Abschnitt ein zweites Tempo gilt; `None` heißt durchgehend.
    tempowechsel: Option<(usize, f64)>,
    teile: &'static [Teil],
    frage: &'static str,
}

/// Die schwierigen Fälle.
///
/// Jeder ist gebaut, um **eine** Annahme zu brechen, die in der Gliederung
/// steckt. Was dabei herauskommt, ist erst dann ein Befund, wenn es gegen die
/// Wahrheitsdatei gehalten wurde — deshalb schreibt dieses Werkzeug beides.
static FAELLE: &[Fall] = &[
    Fall {
        datei: "ohne-outro.wav",
        bpm: 128.0,
        tempowechsel: None,
        // Endet auf dem Höhepunkt. Viele Produktionen tun das; die Regel
        // „der letzte Abschnitt ist ein Outro, wenn er leise ist" darf daraus
        // keins machen.
        teile: &[
            teil("intro", 2, false, false, 0.20),
            teil("aufbau", 2, true, false, 0.30),
            teil("drop", 2, true, true, 0.32),
            teil("break", 2, true, false, 0.12),
            teil("drop", 3, true, true, 0.31),
        ],
        frage: "Endet auf dem Drop. Wird der letzte Abschnitt trotzdem Outro genannt?",
    },
    Fall {
        datei: "ohne-intro.wav",
        bpm: 128.0,
        tempowechsel: None,
        // Fängt sofort oben an — ein Werkzeug-Edit, wie er in jedem
        // DJ-Ordner liegt. `entry` und `intro_beats` hängen daran.
        teile: &[
            teil("drop", 3, true, true, 0.32),
            teil("break", 2, true, false, 0.12),
            teil("drop", 2, true, true, 0.31),
            teil("outro", 2, false, false, 0.18),
        ],
        frage: "Fängt sofort oben an. Wird der erste Abschnitt trotzdem Intro genannt?",
    },
    Fall {
        datei: "zwei-breaks.wav",
        bpm: 128.0,
        tempowechsel: None,
        // Zwei Einbrüche statt einem. Die Benennung arbeitet mit Quantilen
        // über die Abschnitte — bei zwei leisen Stellen verschiebt sich der
        // Median, und das ist genau die Stelle, an der sie schon einmal lag.
        teile: &[
            teil("intro", 2, false, false, 0.20),
            teil("aufbau", 2, true, false, 0.30),
            teil("drop", 2, true, true, 0.33),
            teil("break", 2, true, false, 0.12),
            teil("drop", 2, true, true, 0.31),
            teil("break", 2, true, false, 0.11),
            teil("drop", 2, true, true, 0.32),
            teil("outro", 2, false, false, 0.18),
        ],
        frage: "Zwei Einbrüche. Werden beide Breaks gefunden, oder verschiebt der Median die Namen?",
    },
    Fall {
        datei: "tempowechsel.wav",
        bpm: 124.0,
        // Ab dem vierten Abschnitt schneller — ein Übergang zwischen zwei
        // Stücken im selben File, wie ihn ein Mitschnitt hat.
        tempowechsel: Some((3, 140.0)),
        teile: &[
            teil("intro", 2, false, false, 0.20),
            teil("aufbau", 2, true, false, 0.30),
            teil("drop", 2, true, true, 0.32),
            teil("teil", 2, true, true, 0.30),
            teil("drop", 2, true, true, 0.32),
            teil("outro", 2, false, false, 0.18),
        ],
        frage: "Tempo springt in der Mitte von 124 auf 140. Was macht das Beatgrid — \
und was die Gliederung, die darauf rechnet?",
    },
];

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(ziel) = args.next() else {
        hilfe();
        return Ok(());
    };
    if ziel == "--help" || ziel == "-h" {
        hilfe();
        return Ok(());
    }
    let ziel = PathBuf::from(ziel);
    std::fs::create_dir_all(&ziel).with_context(|| format!("{} anlegen", ziel.display()))?;

    let mut wahrheit = String::from(
        "# Gebautes Material mit bekannter Gliederung — die schwierigen Fälle.\n\
         # Geschrieben von `musik-material`. Die Zeiten sind nicht gehört,\n\
         # sondern gebaut: Sie stimmen auf den Frame.\n\n",
    );

    for fall in FAELLE {
        let (pcm, grenzen) = bauen(fall);
        let pfad = ziel.join(fall.datei);
        schreibe_wav(&pfad, &pcm).with_context(|| format!("{} schreiben", pfad.display()))?;

        wahrheit.push_str(&format!("# {}\n", fall.frage));
        wahrheit.push_str(&format!("{}   bpm {}", fall.datei, fall.bpm));
        for (art, sekunden) in &grenzen {
            wahrheit.push_str(&format!("  {art} {}", zeit(*sekunden)));
        }
        wahrheit.push_str("\n\n");

        println!(
            "{:<18} {:>5.1} s  {} Abschnitte",
            fall.datei,
            pcm.len() as f64 / 2.0 / RATE as f64,
            fall.teile.len()
        );
    }

    let wpfad = ziel.join("wahrheit.txt");
    std::fs::write(&wpfad, wahrheit).with_context(|| format!("{} schreiben", wpfad.display()))?;
    println!("\nWahrheit: {}", wpfad.display());
    println!("Prüfen:   musik-pruefstand {}", wpfad.display());
    Ok(())
}

/// Baut einen Fall und gibt die Grenzen zurück, die dabei entstanden sind.
///
/// Die Grenzen kommen aus dem Bauen selbst und nicht aus einer Rechnung
/// daneben — sonst prüfte die Wahrheitsdatei irgendwann etwas anderes als das,
/// was in der Datei steht.
fn bauen(fall: &Fall) -> (Vec<f32>, Vec<(&'static str, f64)>) {
    let mut mono: Vec<f32> = Vec::new();
    let mut grenzen = Vec::new();
    // Die Phase des Akkords läuft über das ganze Stück weiter. Bräche sie an
    // jeder Teilgrenze, entstünde dort ein Knacks — also ein Onset, den die
    // Gliederung dann zu Recht fände, und die Prüfung misst den Knacks statt
    // der Struktur.
    let mut phase = 0.0f32;

    for (i, t) in fall.teile.iter().enumerate() {
        let bpm = match fall.tempowechsel {
            Some((ab, neu)) if i >= ab => neu,
            _ => fall.bpm,
        };
        let je_beat = 60.0 / bpm * RATE as f64;
        let laenge = (je_beat * PHRASE_BEATS) as usize * t.phrasen;

        grenzen.push((t.art, mono.len() as f64 / RATE as f64));
        let von = mono.len();
        mono.resize(von + laenge, 0.0);

        for wert in mono[von..].iter_mut() {
            for hz in [220.0f32, 277.18, 329.63] {
                *wert += (std::f32::consts::TAU * hz * phase).sin() * t.akkord / 3.0;
            }
            if t.bass {
                *wert += (std::f32::consts::TAU * 55.0 * phase).sin() * 0.5;
            }
            phase += 1.0 / RATE as f32;
        }

        if t.kick {
            let schlag = je_beat as usize;
            let mut start = von;
            while start + RATE as usize / 4 < mono.len() {
                for j in 0..RATE as usize / 4 {
                    let tt = j as f32 / RATE as f32;
                    let f = 110.0 * (-tt * 30.0).exp() + 45.0;
                    mono[start + j] +=
                        (std::f32::consts::TAU * f * tt).sin() * (-tt * 22.0).exp() * 0.9;
                }
                start += schlag;
            }
        }
    }

    let pcm = mono
        .iter()
        .flat_map(|v| [v.clamp(-1.0, 1.0), v.clamp(-1.0, 1.0)])
        .collect();
    (pcm, grenzen)
}

fn zeit(sekunden: f64) -> String {
    let ganze = sekunden.round() as u64;
    format!("{}:{:02}", ganze / 60, ganze % 60)
}

fn schreibe_wav(pfad: &Path, pcm: &[f32]) -> std::io::Result<()> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = BufWriter::new(File::create(pfad)?);

    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&RATE.to_le_bytes())?;
    out.write_all(&(RATE * 4).to_le_bytes())?;
    out.write_all(&4u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;

    for sample in pcm {
        out.write_all(&((sample.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())?;
    }
    out.flush()
}

fn hilfe() {
    println!("Aufruf: musik-material <ordner>");
    println!();
    println!("Baut Prüfmaterial mit bekannter Gliederung und legt die");
    println!("Wahrheitsdatei daneben. Vier Fälle, jeder bricht eine Annahme:");
    for fall in FAELLE {
        println!("  {:<18} {}", fall.datei, fall.frage);
    }
    println!();
    println!("Danach: musik-pruefstand <ordner>/wahrheit.txt");
}
