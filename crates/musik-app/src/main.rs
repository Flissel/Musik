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
mod sammlung;
mod theme;
mod waveform;

use std::path::PathBuf;
use std::sync::Arc;

use analysis::Store;
use anyhow::{Context, Result};
use audio_core::deck::{DeckState, Voice};
use audio_core::{Beatgrid, Track};
use audio_engine::{aux_channel, DeckSource, Engine, EngineRunner, Output};
use control::{DeckEintrag, KanalSpiegel, Server, Steuerpult};
use library::Library;

use app::{assign_fuer, DeckUi, MusikApp, Screenshot};

const RATE: u32 = 48_000;

struct Args {
    db: Option<PathBuf>,
    cache: PathBuf,
    a: Option<PathBuf>,
    b: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    screenshot_nach: std::time::Duration,
    socket: PathBuf,
}

/// Wo die Steuerung lauscht. Im Laufzeitverzeichnis des Benutzers, nicht in
/// `/tmp` — dort könnte jeder andere Benutzer des Rechners mitsteuern.
fn standard_socket() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("musik.sock"),
        None => std::env::temp_dir().join(format!("musik-{}.sock", std::process::id())),
    }
}

fn main() -> Result<()> {
    let args = parse_args()?;

    let mut engine = Engine::new(RATE as f32);
    let cache = Store::new(&args.cache);

    let demo = args.a.is_none() && args.b.is_none();
    let mut decks = Vec::new();
    let mut eintraege = Vec::new();

    // Vor der Deck-Schleife: Ein mit --a geladener Track soll seine Cues von
    // Anfang an haben. Sonst stünde das Deck leer da, und der erste gesetzte
    // Cue überschriebe stillschweigend die gespeicherten.
    let mut library = args.db.as_ref().and_then(|p| Library::open(p).ok());

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
        // Was in der Sammlung steht, schlägt die frische Analyse — dieselbe
        // Regel wie beim Laden zur Laufzeit, siehe `sammlung::fertigen`.
        let gespeichert = pfad
            .as_ref()
            .and_then(|p| gespeichertes(library.as_ref(), &p.to_string_lossy(), RATE));
        let (gespeicherte_cues, gespeichertes_grid) = gespeichert.unwrap_or_default();

        for (nummer, frame) in &gespeicherte_cues {
            state.set_cue(*nummer, Some(*frame));
        }
        state.set_grid(gespeichertes_grid.or_else(|| {
            analyse
                .bpm
                .map(|bpm| Beatgrid::new(bpm, analyse.beat_anchor_frames.unwrap_or(0), 1.0))
        }));

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

        let mut eintrag = DeckEintrag::neu(Arc::clone(&state), kanal, RATE);
        eintrag.frames = frames;
        eintrag.titel = titel;
        eintrag.artist = artist;
        eintrag.tonart = analyse.tonart();
        // Nur bei echten Dateien: Ein Demo-Track steht in keiner Sammlung, und
        // ein Cue darauf hat nichts, wohin er gespeichert werden könnte.
        if let Some(p) = pfad {
            eintrag.pfad = p.to_string_lossy().into_owned();
        }
        eintraege.push((eintrag, fader, name));

        decks.push(DeckUi {
            name: name.to_string(),
            state,
            peaks: analyse.peaks.iter().filter_map(|p| p.to_level()).collect(),
            frames,
            sample_rate: RATE,
            zoom_secs: 8.0,
            loop_beats: 4.0,
        });
    }

    // AUX bleibt ohne Zuspieler still, ist aber vorhanden und bedienbar.
    let (_aux_writer, aux_source) = aux_channel(RATE as usize * 2);
    let aux_kanal = engine.add_channel("AUX", Box::new(aux_source));
    engine.channel(aux_kanal).set_assign(assign_fuer(2));

    // Der Mitschnitt greift hinter dem Begrenzer ab. Angelegt vor der
    // Kommandoschlange, weil die Engine danach in den Audio-Thread wandert.
    let (tap, aufnahme) = audio_engine::mitschnitt(RATE);
    engine.set_mitschnitt(tap);

    let (handle, runner) = audio_engine::engine_channel(engine, 512);

    // Das Steuerpult bekommt alles, was bedienbar ist — und ist danach die
    // einzige Stelle, an der Reglerstellungen stehen.
    let mut pult = Steuerpult::neu(handle);
    for (eintrag, fader, name) in eintraege {
        let assign = assign_fuer(pult.decks().len());
        let mut spiegel = KanalSpiegel::neu(name, assign);
        spiegel.fader = fader as f64;
        pult.kanal_hinzufuegen(spiegel);
        pult.deck_hinzufuegen(eintrag);
    }
    pult.kanal_hinzufuegen(KanalSpiegel::neu("AUX", assign_fuer(2)));

    // Suchen und Laden hängen das Pult an die Sammlung und an den Dekodierer.
    // Ohne das antworten `search` und `load` mit einem Fehler, statt
    // stillschweigend nichts zu tun.
    let deck_zustaende: Vec<_> = pult
        .decks()
        .iter()
        .map(|d| (Arc::clone(&d.state), d.sample_rate))
        .collect();
    let (sammlung, ergebnisse) =
        sammlung::AppSammlung::neu(library.take(), deck_zustaende, args.cache.clone());
    pult.sammlung_setzen(Box::new(sammlung));
    pult.aufnahme_setzen(aufnahme);
    // Die erste Liste kommt über denselben Weg wie jede spätere Suche.
    let treffer = pult.suche("");
    let playlisten = pult.playlists();

    let pult = Arc::new(std::sync::Mutex::new(pult));

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

    // Ab hier ist die Anlage von außen bedienbar. Scheitert das, läuft die
    // Oberfläche trotzdem — nur eben allein.
    let steuerung = match Server::starten(&args.socket, Arc::clone(&pult)) {
        Ok(server) => {
            println!("Steuerung: {}", server.pfad().display());
            Some(server)
        }
        Err(e) => {
            eprintln!("Steuerung nicht verfügbar: {e}");
            None
        }
    };

    let anwendung = MusikApp {
        ergebnisse,
        pult,
        output,
        audio_hinweis: hinweis,
        decks,
        suche: String::new(),
        treffer,
        playlisten,
        playliste: String::new(),
        status: if demo {
            "Demobetrieb mit synthetischen Tracks".into()
        } else {
            String::new()
        },
        screenshot: args.screenshot.map(|pfad| Screenshot {
            pfad,
            warte_bis: std::time::Instant::now() + args.screenshot_nach,
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
    .map_err(|e| anyhow::anyhow!("Fenster ließ sich nicht öffnen: {e}"))?;

    // Erst hier fallen lassen: Der Socket wird beim Aufräumen entfernt, und
    // das soll nicht schon passieren, während das Fenster noch steht.
    drop(steuerung);
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        db: None,
        cache: PathBuf::from(".musik-analyse"),
        a: None,
        b: None,
        screenshot: None,
        // Lang genug, dass das Fenster steht und die erste Wellenform
        // gezeichnet ist.
        screenshot_nach: std::time::Duration::from_millis(600),
        socket: standard_socket(),
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
            "--socket" => args.socket = PathBuf::from(wert()?),
            "--screenshot-nach" => {
                let sekunden: f64 = wert()?
                    .parse()
                    .context("--screenshot-nach braucht Sekunden")?;
                args.screenshot_nach = std::time::Duration::from_secs_f64(sekunden.max(0.0));
            }
            "-h" | "--help" => {
                println!("Aufruf: musik-app [--db <datei>] [--a <track>] [--b <track>]");
                println!("                  [--cache <dir>] [--screenshot <bild.png>]");
                println!("                  [--socket <pfad>] [--screenshot-nach <sekunden>]");
                std::process::exit(0);
            }
            other => anyhow::bail!("unbekannte Option: {other}"),
        }
    }

    Ok(args)
}

/// Was die Sammlung einem Deck mitgibt: Hot Cues als (Nummer, Frame) und ein
/// Beatgrid.
type Gespeichertes = (Vec<(usize, u64)>, Option<Beatgrid>);

/// Hot Cues und Beatgrid, die für diesen Pfad in der Sammlung stehen.
///
/// Dasselbe, was `sammlung::AppSammlung` beim Laden zur Laufzeit tut — hier
/// noch einmal, weil die Decks beim Start an der Sammlung vorbei geladen
/// werden.
fn gespeichertes(library: Option<&Library>, pfad: &str, rate: u32) -> Option<Gespeichertes> {
    let lib = library?;
    let eintrag = lib.track_by_path(pfad).ok().flatten()?;
    let grid = eintrag.beatgrid(rate);

    let cues = eintrag
        .id
        .and_then(|id| lib.cues(id).ok())
        .map(|zeilen| {
            zeilen
                .iter()
                .filter_map(|c| {
                    let nummer = c.hotcue? as usize;
                    (nummer < audio_core::deck::HOT_CUES).then(|| (nummer, c.frame(rate)))
                })
                .collect()
        })
        .unwrap_or_default();

    Some((cues, grid))
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
