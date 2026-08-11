//! Analysiert Tracks und legt die Ergebnisse als Sidecar ab.
//!
//! Läuft komplett ohne Audiogerät — Tempo, Beatgrid und Wellenform-Spitzen
//! entstehen offline. Das ist der Schritt, der Phase 2 aus `docs/PLAN.md`
//! bedient und die Grundlage für Sync und Waveform-Anzeige legt.

use std::path::{Path, PathBuf};

use analysis::{Store, analyze_cached};
use anyhow::{Context, Result};
use audio_core::Track;

const DEFAULT_CACHE: &str = ".musik-analyse";

fn main() -> Result<()> {
    let mut cache = PathBuf::from(DEFAULT_CACHE);
    let mut dateien: Vec<PathBuf> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cache" => {
                let dir = args.next().context("--cache braucht ein Verzeichnis")?;
                cache = PathBuf::from(dir);
            }
            "-h" | "--help" => {
                hilfe();
                return Ok(());
            }
            other => dateien.push(PathBuf::from(other)),
        }
    }

    if dateien.is_empty() {
        hilfe();
        std::process::exit(2);
    }

    let store = Store::new(&cache);
    println!("Cache: {}", store.root().display());

    let mut fehler = 0;
    for pfad in &dateien {
        if let Err(e) = eine_datei(pfad, &store) {
            eprintln!("{}: {e:#}", pfad.display());
            fehler += 1;
        }
    }

    if fehler > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn eine_datei(pfad: &Path, store: &Store) -> Result<()> {
    let track = Track::decode_file(pfad)
        .with_context(|| format!("konnte {} nicht dekodieren", pfad.display()))?;

    let (analyse, gerechnet) = analyze_cached(&track, store);
    if gerechnet {
        store
            .save(&analyse)
            .context("Sidecar konnte nicht geschrieben werden")?;
    }

    let tempo = match (analyse.bpm, analyse.beat_anchor_frames) {
        (Some(bpm), Some(anker)) => {
            let sek = anker as f64 / analyse.sample_rate as f64;
            let konfidenz = analyse.bpm_confidence.unwrap_or(0.0);
            format!("{bpm:6.2} BPM   Anker {sek:6.3}s   Konfidenz {konfidenz:.2}")
        }
        _ => "kein verlässliches Tempo erkannt".to_string(),
    };

    let spitzen: Vec<String> = analyse
        .peaks
        .iter()
        .map(|p| format!("{}", p.samples_per_peak))
        .collect();

    println!(
        "\n{}\n  {}\n  {}\n  Dauer {:.1}s   Spitzen-Stufen {}   {}",
        pfad.display(),
        analyse.fingerprint,
        tempo,
        analyse.duration_secs,
        spitzen.join("/"),
        if gerechnet {
            "berechnet"
        } else {
            "aus dem Cache"
        },
    );

    Ok(())
}

fn hilfe() {
    println!("Aufruf: musik-analyze [--cache <verzeichnis>] <datei>...");
    println!();
    println!("Analysiert Tempo, Beatgrid und Wellenform-Spitzen und legt sie als");
    println!("Sidecar ab, adressiert über einen Hash des Audioinhalts. Ein erneuter");
    println!("Lauf über dieselbe Datei liest aus dem Cache.");
    println!();
    println!("  --cache <dir>   Ablageort der Sidecars (Vorgabe: {DEFAULT_CACHE})");
}
