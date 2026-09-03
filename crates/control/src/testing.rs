//! Ein vollständig verkabeltes Pult für Tests.
//!
//! Mit echter Engine und echter Kommandoschlange, nur ohne Audiogerät. Der
//! `EngineRunner` muss dabei am Leben bleiben: Fällt er weg, bricht die
//! Schlange, und jedes gesendete Kommando liefe ins Nichts — die Tests würden
//! grün, obwohl nichts ankommt.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use audio_core::deck::DeckState;
use audio_engine::{Assign, DeckSource, Engine, EngineRunner, SilentSource};

use crate::pult::{DeckEintrag, KanalSpiegel, Sammlung, Steuerpult, Treffer};

pub const RATE: u32 = 48_000;

/// Was die Sammlung an Hot Cues zu sehen bekommen hat, je Pfad.
///
/// Geteilt, weil die Sammlung im Pult unter einem `Box<dyn Sammlung>`
/// verschwindet — ohne einen Griff von außen ließe sich nicht prüfen, ob ein
/// Cue wirklich zurückgeschrieben wurde.
pub type CueProtokoll = Arc<Mutex<HashMap<String, Vec<(usize, f64)>>>>;

/// Das zuletzt zurückgeschriebene Beatgrid: Pfad, Tempo, Anker in Sekunden.
pub type GridProtokoll = Arc<Mutex<Option<(String, f32, f64)>>>;

/// Zwei Decks auf Kanal 1 und 2, dazu ein AUX-Kanal auf Thru.
pub fn pult_mit_zwei_decks() -> (Steuerpult, EngineRunner) {
    let (pult, runner, _, _) = pult_mit_protokoll();
    (pult, runner)
}

/// Dasselbe Pult, aber mit einem Blick auf das, was zurückgeschrieben wird.
pub fn pult_mit_protokoll() -> (Steuerpult, EngineRunner, CueProtokoll, GridProtokoll) {
    let mut engine = Engine::new(RATE as f32);

    // Echte Abspieler statt `SilentSource`: Nur so bewegt sich die Position,
    // und ohne Position gibt es keine Beats — der Zeitplan wäre nicht prüfbar.
    // Gerechnet wird trotzdem nur, wenn ein Test den Runner ausdrücklich
    // dreht; stillstehende Tests bleiben stillstehend.
    let mut kanaele = Vec::new();
    let mut zustaende = Vec::new();
    for (name, assign) in [("DECK A", Assign::A), ("DECK B", Assign::B)] {
        let state = Arc::new(DeckState::new());
        let track = Arc::new(audio_core::Track {
            samples: vec![0.0; RATE as usize * 60 * 2],
            sample_rate: RATE,
            stems: Vec::new(),
        });
        let voice = audio_core::deck::Voice::new(track, Arc::clone(&state));
        let kanal = engine.add_channel(name, Box::new(DeckSource::new(voice)));
        engine.channel(kanal).set_assign(assign);
        kanaele.push((name, kanal, assign));
        zustaende.push(state);
    }
    let aux = engine.add_channel("AUX", Box::new(SilentSource));
    engine.channel(aux).set_assign(Assign::Thru);

    // Der Mitschnitt hängt an der Engine, genau wie im Ernstfall — sonst
    // liefe der Test gegen ein Loch, wo in der Anwendung ein Ringpuffer ist.
    let (tap, aufnahme) = audio_engine::mitschnitt(RATE);
    engine.set_mitschnitt(tap);

    let (handle, runner) = audio_engine::engine_channel(engine, 256);
    let mut pult = Steuerpult::neu(handle);

    for ((name, kanal, assign), state) in kanaele.iter().zip(zustaende) {
        pult.kanal_hinzufuegen(KanalSpiegel::neu(*name, *assign));
        let mut eintrag = DeckEintrag::neu(state, *kanal, RATE);
        // Eine Minute, damit Positionen und Hot Cues Platz haben.
        eintrag.frames = RATE as u64 * 60;
        eintrag.titel = format!("Testtrack {name}");
        eintrag.artist = "Test".into();
        eintrag.tonart = audio_core::Tonart::parse("Am");
        pult.deck_hinzufuegen(eintrag);
    }
    pult.kanal_hinzufuegen(KanalSpiegel::neu("AUX", Assign::Thru));

    // Ein Deck ohne Grid könnte `loop_beats` nicht setzen, und der Test, der
    // jedes schreibbare Control durchgeht, würde das nicht bemerken.
    for eintrag in pult.decks() {
        eintrag
            .state
            .set_grid(Some(audio_core::Beatgrid::new(128.0, 0, 1.0)));
    }

    let protokoll: CueProtokoll = Arc::new(Mutex::new(HashMap::new()));
    let grid_protokoll: GridProtokoll = Arc::new(Mutex::new(None));
    pult.sammlung_setzen(Box::new(TestSammlung {
        gesicherte_cues: Arc::clone(&protokoll),
        gesichertes_grid: Arc::clone(&grid_protokoll),
    }));

    pult.aufnahme_setzen(aufnahme);
    (pult, runner, protokoll, grid_protokoll)
}

/// Eine Sammlung, die nichts liest und nichts dekodiert.
///
/// Sie hält fest, dass ein Ladeauftrag *angenommen* wurde — mehr verspricht
/// die Schnittstelle nicht, und mehr soll ein Test hier auch nicht prüfen.
pub struct TestSammlung {
    /// Was zurückgeschrieben wurde — damit ein Test das nachsehen kann.
    pub gesicherte_cues: CueProtokoll,
    pub gesichertes_grid: GridProtokoll,
}

impl Sammlung for TestSammlung {
    fn suchen(&self, text: &str, grenze: usize) -> Vec<Treffer> {
        // Ein Spektrum von ruhig bis heftig, und einer ohne Analyse: Die
        // Auswahl nach Energie muss sich an beidem prüfen lassen.
        const ENERGIEN: [Option<f64>; 3] = [Some(0.2), Some(0.55), None];
        (0..3.min(grenze))
            .map(|i| Treffer {
                pfad: format!("/musik/{text}-{i}.wav"),
                titel: format!("{text} {i}"),
                artist: None,
                bpm: Some(128.0 + i as f32),
                tonart: audio_core::Tonart::parse("Am"),
                energie: ENERGIEN[i],
            })
            .collect()
    }

    fn suchen_mischbar(&self, bpm: f32, grenze: usize) -> Vec<Treffer> {
        self.suchen(&format!("{bpm:.0}"), grenze)
    }

    fn suchen_harmonisch(&self, tonart: audio_core::Tonart, grenze: usize) -> Vec<Treffer> {
        self.suchen(&tonart.camelot(), grenze)
    }

    fn playlists(&self) -> Vec<String> {
        vec!["Freitag".into(), "Warmup".into()]
    }

    fn playlist(&self, name: &str, grenze: usize) -> Vec<Treffer> {
        if name == "Freitag" {
            self.suchen(name, grenze)
        } else {
            Vec::new()
        }
    }

    fn laden(&self, _deck: usize, pfad: &str) -> Result<(), String> {
        if pfad.ends_with(".txt") {
            return Err("keine Audiodatei".into());
        }
        Ok(())
    }

    fn grid_speichern(&self, pfad: &str, bpm: f32, anker_sekunden: f64) -> Result<(), String> {
        if pfad.ends_with(".schreibgeschuetzt") {
            return Err("nicht beschreibbar".into());
        }
        *self.gesichertes_grid.lock().unwrap() = Some((pfad.to_string(), bpm, anker_sekunden));
        Ok(())
    }

    fn hot_cues_speichern(&self, pfad: &str, cues: &[(usize, f64)]) -> Result<(), String> {
        if pfad.ends_with(".schreibgeschuetzt") {
            return Err("nicht beschreibbar".into());
        }
        self.gesicherte_cues
            .lock()
            .unwrap()
            .insert(pfad.to_string(), cues.to_vec());
        Ok(())
    }
}

/// Dreht den Mixer so lange, bis so viele Frames vergangen sind.
///
/// Die Decks laufen sonst nicht: Position und Beats bewegen sich nur, wenn
/// jemand rendert. Im Ernstfall ist das die Soundkarte, hier der Test.
pub fn rendern(runner: &mut EngineRunner, frames: usize) {
    const BLOCK: usize = 256;
    let mut puffer = vec![0.0f32; BLOCK * 4];
    let mut getan = 0;
    while getan < frames {
        runner.render(&mut puffer, 4);
        getan += BLOCK;
    }
}
