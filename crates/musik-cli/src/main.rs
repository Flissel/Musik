//! Phase-0-Prototyp: ein Deck an der Kommandozeile.
//!
//! Zweck ist nicht Bedienkomfort, sondern die Frage aus `docs/BAUSTEINE.md`
//! empirisch zu beantworten — trägt der native Audio-Pfad, und wie klingt die
//! Zeitstreckung bei den ±8 %, die im DJ-Betrieb üblich sind?

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use audio_core::{Player, Track};

fn main() -> Result<()> {
    let path: PathBuf = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("Aufruf: musik-cli <audiodatei>");
            eprintln!("Unterstützt: mp3, flac, wav, m4a/aac, ogg");
            std::process::exit(2);
        }
    };

    println!("Lade {} …", path.display());
    let track = Track::decode_file(&path)
        .with_context(|| format!("konnte {} nicht dekodieren", path.display()))?;
    println!(
        "  {} Frames, {} Hz, {}",
        track.frames(),
        track.sample_rate,
        format_time(track.duration_secs())
    );

    let player = Player::open(track).context("konnte das Ausgabegerät nicht öffnen")?;
    println!(
        "Gerät: {} @ {} Hz",
        player.device_name(),
        player.sample_rate()
    );
    println!();
    print_help();

    let state = player.state().clone();
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        print!("> ");
        io::stdout().flush().ok();

        let Some(line) = lines.next() else { break };
        let line = line?;
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else { continue };
        let arg = parts.next();

        match cmd {
            "p" | "play" => {
                let now = state.toggle_playing();
                println!("{}", if now { "▶ läuft" } else { "⏸ pausiert" });
            }
            "k" | "keylock" => {
                let next = !state.keylock();
                state.set_keylock(next);
                println!("Keylock {}", if next { "an" } else { "aus" });
            }
            "t" | "tempo" => match arg.map(parse_tempo) {
                Some(Ok(ratio)) => {
                    state.set_tempo(ratio);
                    println!(
                        "Tempo {:+.2} % ({:.4}×)",
                        (state.tempo() - 1.0) * 100.0,
                        state.tempo()
                    );
                }
                Some(Err(e)) => println!("{e}"),
                None => println!("Tempo {:+.2} %", (state.tempo() - 1.0) * 100.0),
            },
            "s" | "seek" => match arg.map(|a| a.parse::<f64>()) {
                Some(Ok(secs)) => {
                    player.seek_secs(secs);
                    println!("Sprung auf {}", format_time(secs));
                }
                _ => println!("Aufruf: s <sekunden>"),
            },
            "i" | "info" => print_status(&player),
            "h" | "help" | "?" => print_help(),
            "q" | "quit" | "exit" => break,
            other => println!("unbekannt: {other} — 'h' für Hilfe"),
        }

        if state.is_finished() {
            println!("(Track zu Ende)");
        }
    }

    Ok(())
}

/// Nimmt entweder ein Verhältnis (`1.06`) oder eine Prozentangabe (`+6`, `-3`).
fn parse_tempo(raw: &str) -> std::result::Result<f32, String> {
    let value: f32 = raw.parse().map_err(|_| format!("'{raw}' ist keine Zahl"))?;

    let ratio = if raw.starts_with('+') || raw.starts_with('-') {
        1.0 + value / 100.0
    } else {
        value
    };

    if !(0.25..=4.0).contains(&ratio) {
        return Err(format!("{ratio:.3}× liegt außerhalb von 0.25×–4.0×"));
    }
    Ok(ratio)
}

fn print_status(player: &Player) {
    let state = player.state();
    println!(
        "{}  {}  Tempo {:+.2} %  Keylock {}",
        if state.is_playing() { "▶" } else { "⏸" },
        format_time(player.position_secs()),
        (state.tempo() - 1.0) * 100.0,
        if state.keylock() { "an" } else { "aus" }
    );
}

fn print_help() {
    println!("  p          Play/Pause");
    println!("  t <wert>   Tempo — '1.06' als Verhältnis oder '+6' in Prozent");
    println!("  k          Keylock an/aus (Tonhöhe halten beim Tempowechsel)");
    println!("  s <sek>    Springen");
    println!("  i          Status");
    println!("  q          Beenden");
}

fn format_time(secs: f64) -> String {
    let secs = secs.max(0.0);
    let m = (secs / 60.0).floor() as u64;
    let s = secs - (m as f64) * 60.0;
    format!("{m}:{s:05.2}")
}
