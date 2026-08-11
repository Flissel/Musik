//! Die Sammlung von der Kommandozeile: einlesen, importieren, durchsuchen.
//!
//! Führt Analyse und Library zusammen. Beim Einlesen wird jede Datei dekodiert,
//! analysiert und mit Tempo, Beatgrid und Inhalts-Hash abgelegt — der Hash
//! sorgt dafür, dass ein zweiter Lauf nicht noch einmal rechnet.
//!
//! Braucht **kein** Audiogerät.

use std::path::{Path, PathBuf};

use analysis::Store;
use anyhow::{Context, Result, bail};
use audio_core::Track;
use library::{Library, Query, TrackRecord, import_nml};

const AUDIO_EXT: [&str; 7] = ["mp3", "flac", "wav", "m4a", "aac", "ogg", "oga"];

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut db = PathBuf::from("musik.db");
    let mut cache = PathBuf::from(".musik-analyse");
    let mut befehl: Option<String> = None;
    let mut rest: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = PathBuf::from(args.next().context("--db braucht einen Pfad")?),
            "--cache" => cache = PathBuf::from(args.next().context("--cache braucht einen Pfad")?),
            "-h" | "--help" => {
                hilfe();
                return Ok(());
            }
            other if befehl.is_none() => befehl = Some(other.to_string()),
            other => rest.push(other.to_string()),
        }
    }

    let Some(befehl) = befehl else {
        hilfe();
        std::process::exit(2);
    };

    let lib = Library::open(&db).with_context(|| format!("{} nicht nutzbar", db.display()))?;
    println!("Sammlung: {} ({} Tracks)", db.display(), lib.track_count()?);

    match befehl.as_str() {
        "scan" => {
            let ordner = rest.first().context("scan braucht ein Verzeichnis")?;
            scan(&lib, Path::new(ordner), &Store::new(&cache))?
        }
        "import-traktor" => {
            let datei = rest.first().context("import-traktor braucht eine .nml")?;
            let xml =
                std::fs::read_to_string(datei).with_context(|| format!("{datei} nicht lesbar"))?;
            let bericht = import_nml(&xml, &lib)?;

            println!(
                "\n{} Einträge gelesen, {} übernommen, {} Marker, {} ohne Pfad übersprungen",
                bericht.entries_seen,
                bericht.tracks_imported,
                bericht.cues_imported,
                bericht.skipped_without_location
            );
        }
        "search" => {
            let text = rest.join(" ");
            let treffer = lib.search(&Query::text(text))?;
            zeige(&treffer);
        }
        "mixable" => {
            let bpm: f32 = rest
                .first()
                .context("mixable braucht ein Tempo")?
                .parse()
                .context("Tempo ist keine Zahl")?;
            let treffer = lib.search(&Query::mixable_with(bpm, 0.06))?;
            println!("\nMischbar mit {bpm:.1} BPM (±6 %):");
            zeige(&treffer);
        }
        "missing-attribution" => {
            let luecken = lib.tracks_missing_attribution()?;
            if luecken.is_empty() {
                println!("\nAlle Samples haben Lizenz und Urheber.");
            } else {
                println!(
                    "\n{} Samples ohne vollständige Rechteangabe:",
                    luecken.len()
                );
                zeige(&luecken);
            }
        }
        other => bail!("unbekannter Befehl: {other}"),
    }

    Ok(())
}

fn scan(lib: &Library, ordner: &Path, store: &Store) -> Result<()> {
    let dateien = sammle_dateien(ordner)?;
    println!("{} Audiodateien gefunden\n", dateien.len());

    let mut neu = 0usize;
    let mut fehler = 0usize;

    for pfad in &dateien {
        match einlesen(lib, pfad, store) {
            Ok(gerechnet) => {
                if gerechnet {
                    neu += 1;
                }
            }
            Err(e) => {
                eprintln!("  {}: {e:#}", pfad.display());
                fehler += 1;
            }
        }
    }

    println!(
        "\n{} eingelesen, {} neu analysiert, {} Fehler",
        dateien.len(),
        neu,
        fehler
    );
    Ok(())
}

/// Gibt zurück, ob gerechnet werden musste.
fn einlesen(lib: &Library, pfad: &Path, store: &Store) -> Result<bool> {
    let track = Track::decode_file(pfad)?;
    let (analyse, gerechnet) = analysis::analyze_cached(&track, store);
    if gerechnet {
        store.save(&analyse)?;
    }

    let mut eintrag = TrackRecord::from_path(pfad.to_string_lossy().to_string());
    eintrag.fingerprint = Some(analyse.fingerprint.clone());
    eintrag.duration_secs = Some(analyse.duration_secs);
    eintrag.title = pfad.file_stem().map(|s| s.to_string_lossy().to_string());

    if let (Some(bpm), Some(anker)) = (analyse.bpm, analyse.beat_anchor_frames) {
        eintrag.set_beatgrid(
            Some(audio_core::Beatgrid::new(bpm, anker, 1.0)),
            analyse.sample_rate,
        );
    }

    lib.upsert_track(&eintrag)?;

    let tempo = match analyse.bpm {
        Some(bpm) => format!("{bpm:6.2} BPM"),
        None => "  kein Tempo".to_string(),
    };
    println!(
        "  {tempo}  {}  {}",
        if gerechnet { "neu " } else { "Cache" },
        pfad.display()
    );

    Ok(gerechnet)
}

fn sammle_dateien(ordner: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut offen = vec![ordner.to_path_buf()];

    while let Some(dir) = offen.pop() {
        let Ok(eintraege) = std::fs::read_dir(&dir) else {
            continue;
        };
        for eintrag in eintraege.flatten() {
            let pfad = eintrag.path();
            if pfad.is_dir() {
                offen.push(pfad);
            } else if pfad
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| AUDIO_EXT.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false)
            {
                out.push(pfad);
            }
        }
    }

    out.sort();
    Ok(out)
}

fn zeige(tracks: &[TrackRecord]) {
    if tracks.is_empty() {
        println!("  (nichts gefunden)");
        return;
    }
    for t in tracks {
        let bpm = t
            .bpm
            .map(|b| format!("{b:6.2}"))
            .unwrap_or_else(|| "     -".into());
        println!(
            "  {bpm}  {:<28}  {}",
            t.artist.as_deref().unwrap_or("—"),
            t.title.as_deref().unwrap_or(&t.path)
        );
    }
}

fn hilfe() {
    println!("Aufruf: musik-lib [--db <datei>] [--cache <dir>] <befehl> [argumente]");
    println!();
    println!("  scan <ordner>            Audiodateien einlesen, analysieren und ablegen");
    println!("  import-traktor <nml>     Traktor-Sammlung übernehmen (Tempo, Grid, Cues)");
    println!("  search <text>            Titel, Künstler und Album durchsuchen");
    println!("  mixable <bpm>            Tracks im Tempofenster ±6 %");
    println!("  missing-attribution      Samples ohne Lizenz oder Urheber auflisten");
    println!();
    println!("  --db <datei>             Sammlung (Vorgabe: musik.db)");
    println!("  --cache <dir>            Analyse-Sidecars (Vorgabe: .musik-analyse)");
}
