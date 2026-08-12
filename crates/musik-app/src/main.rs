//! Die Oberfläche des DJ-Werkzeugs.
//!
//! ```text
//! musik-app                          # mit synthetischen Demo-Tracks
//! musik-app --db musik.db            # mit der eigenen Sammlung
//! musik-app --a a.mp3 --b b.mp3      # zwei Dateien direkt auf die Decks
//! musik-app --screenshot bild.png    # ein Bild aufnehmen und beenden
//! ```
//!
//! Ohne Audiogerät läuft alles trotzdem: Der Mixer wird dann von einem
//! Taktgeber im Leerlauf gerendert, sodass die Decks sich bewegen und die
//! Oberfläche sich beurteilen lässt. Nur hören kann man nichts.

mod app;
mod demo;
mod laden;
mod theme;
mod waveform;

use std::path::PathBuf;
use std::sync::Arc;

use analysis::Store;
use anyhow::{Context, Result};
use audio_core::deck::{DeckState, Voice};
use audio_core::{Beatgrid, Track};
use audio_engine::{aux_channel, DeckSource, Engine, EngineRunner, Output};
use library::Library;

use app::{assign_fuer, ChannelUi, DeckUi, MusikApp, Screenshot};

const RATE: u32 = 48_000;

struct Args {
    db: Option<PathBuf>,
    cache: PathBuf,
    a: Option<PathBuf>,
    b: Option<PathBuf>,
    screenshot: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = parse_args()?;

    let mut engine = Engine::new(RATE as f32);
    let cache = Store::new(&args.cache);

    let demo = args.a.is_none() && args.b.is_none();
    let mut decks = Vec::new();

    for (index, (name, pfad)) in [("DECK A", &args.a), ("DECK B", &args.b)]
        .into_iter()
        .enumerate()
    {
        let (track, artist, titel) = match pfad {
            Some(p) => {
                let track = Track::decode_file(p)
                    .with_context(|| format!("{} nicht lesbar", p.display()))?
                    .resampled_to(RATE);
                let titel = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                (track, String::new(), titel)
            }
            None => {
                let d = if index == 0 {
                    demo::deck_a(RATE)
                } else {
                    demo::deck_b(RATE)
                };
                (d.track, d.artist, d.title)
            }
        };

        let (analyse, gerechnet) = analysis::analyze_cached(&track, &cache);
        if gerechnet && pfad.is_some() {
            let _ = cache.save(&analyse);
        }

        let state = Arc::new(DeckState::new());
        state.set_keylock(true);
        state.set_grid(
            analyse
                .bpm
                .map(|bpm| Beatgrid::new(bpm, analyse.beat_anchor_frames.unwrap_or(0), 1.0)),
        );

        let frames = track.frames() as u64;
        let voice = Voice::new(Arc::new(track), Arc::clone(&state));
        let kanal = engine.add_channel(name, Box::new(DeckSource::new(voice)));
        engine.channel(kanal).set_assign(assign_fuer(index));

        // Im Demobetrieb laufen die Decks sofort, damit es etwas zu sehen gibt.
        // Mit eigenen Dateien bleibt der Fader unten — ein Deck, das beim
        // Start auf die Anlage geht, ist ein Unfall.
        let fader = if demo { 0.8 } else { 0.0 };
        engine.channel(kanal).set_fader(fader);
        state.set_playing(demo);

        let mut strip = ChannelUi::new(name, kanal);
        strip.fader = fader;

        decks.push(DeckUi {
            name: name.to_string(),
            state,
            artist,
            titel,
            peaks: analyse.peaks.iter().filter_map(|p| p.to_level()).collect(),
            frames,
            sample_rate: RATE,
            strip,
            zoom_secs: 8.0,
            loop_beats: 4.0,
        });
    }

    // AUX bleibt ohne Zuspieler still, ist aber vorhanden und bedienbar.
    let (_aux_writer, aux_source) = aux_channel(RATE as usize * 2);
    let aux_kanal = engine.add_channel("AUX", Box::new(aux_source));
    engine.channel(aux_kanal).set_assign(assign_fuer(2));

    let (handle, runner) = audio_engine::engine_channel(engine, 512);

    let (output, hinweis) = match Output::open(runner) {
        Ok(out) => {
            let cue = if out.has_cue_output() {
                "Cue auf 3/4".to_string()
            } else {
                format!("nur {} Kanäle — kein Vorhören", out.channels())
            };
            let text = format!("{} · {} Hz · {}", out.device_name(), out.sample_rate(), cue);
            (Some(out), text)
        }
        Err(fehler) => {
            let text = format!("kein Audiogerät ({fehler}) — Trockenlauf");
            // Der Mixer läuft weiter, nur eben ins Leere — sonst stünde die
            // Oberfläche still und wäre nicht zu beurteilen.
            trockenlauf(fehler.runner);
            (None, text)
        }
    };

    let library = args.db.as_ref().and_then(|p| Library::open(p).ok());
    let treffer = library
        .as_ref()
        .and_then(|l| l.search(&library::Query::default()).ok())
        .unwrap_or_default();

    let anwendung = MusikApp {
        handle,
        output,
        audio_hinweis: hinweis,
        decks,
        aux: ChannelUi::new("AUX", aux_kanal),
        crossfader: 0.0,
        crossfader_kurve: 0.0,
        master_gain: 1.0,
        cue_mix: 0.0,
        cue_gain: 1.0,
        library,
        analyse_cache: args.cache.clone(),
        suche: String::new(),
        treffer,
        status: if demo {
            "Demobetrieb mit synthetischen Tracks".into()
        } else {
            String::new()
        },
        screenshot: args.screenshot.map(|pfad| Screenshot {
            pfad,
            warte_bilder: 30,
            angefordert: false,
        }),
    };

    let optionen = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([1100.0, 760.0])
            .with_title("Musik"),
        ..Default::default()
    };

    eframe::run_native(
        "Musik",
        optionen,
        Box::new(|cc| {
            theme::stil(&cc.egui_ctx);
            Ok(Box::new(anwendung))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Fenster ließ sich nicht öffnen: {e}"))
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        db: None,
        cache: PathBuf::from(".musik-analyse"),
        a: None,
        b: None,
        screenshot: None,
    };

    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut wert = || iter.next().context("fehlender Wert");
        match arg.as_str() {
            "--db" => args.db = Some(PathBuf::from(wert()?)),
            "--cache" => args.cache = PathBuf::from(wert()?),
            "--a" => args.a = Some(PathBuf::from(wert()?)),
            "--b" => args.b = Some(PathBuf::from(wert()?)),
            "--screenshot" => args.screenshot = Some(PathBuf::from(wert()?)),
            "-h" | "--help" => {
                println!("Aufruf: musik-app [--db <datei>] [--a <track>] [--b <track>]");
                println!("                  [--cache <dir>] [--screenshot <bild.png>]");
                std::process::exit(0);
            }
            other => anyhow::bail!("unbekannte Option: {other}"),
        }
    }

    Ok(args)
}

/// Rendert den Mixer im Leerlauf, wenn kein Audiogerät da ist.
///
/// Ohne das stünden die Decks still und die Oberfläche wäre ein Standbild —
/// gerade in einer Umgebung ohne Soundkarte wäre sie dann nicht zu beurteilen.
fn trockenlauf(mut runner: EngineRunner) {
    std::thread::spawn(move || {
        const BLOCK: usize = 1_024;
        let mut puffer = vec![0.0f32; BLOCK * 4];
        let takt = std::time::Duration::from_secs_f64(BLOCK as f64 / RATE as f64);

        loop {
            runner.render(&mut puffer, 4);
            std::thread::sleep(takt);
        }
    });
}
