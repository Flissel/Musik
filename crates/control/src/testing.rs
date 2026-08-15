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

    let mut kanaele = Vec::new();
    for (name, assign) in [("DECK A", Assign::A), ("DECK B", Assign::B)] {
        let kanal = engine.add_channel(name, Box::new(SilentSource));
        engine.channel(kanal).set_assign(assign);
        kanaele.push((name, kanal, assign));
    }
    let aux = engine.add_channel("AUX", Box::new(SilentSource));
    engine.channel(aux).set_assign(Assign::Thru);

    // Der Mitschnitt hängt an der Engine, genau wie im Ernstfall — sonst
    // liefe der Test gegen ein Loch, wo in der Anwendung ein Ringpuffer ist.
    let (tap, aufnahme) = audio_engine::mitschnitt(RATE);
    engine.set_mitschnitt(tap);

    let (handle, runner) = audio_engine::engine_channel(engine, 256);
    let mut pult = Steuerpult::neu(handle);

    for (name, kanal, assign) in &kanaele {
        pult.kanal_hinzufuegen(KanalSpiegel::neu(*name, *assign));
        let mut eintrag = DeckEintrag::neu(Arc::new(DeckState::new()), *kanal, RATE);
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

    let _ = DeckSource::new as fn(_) -> _;
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
        (0..3.min(grenze))
            .map(|i| Treffer {
                pfad: format!("/musik/{text}-{i}.wav"),
                titel: format!("{text} {i}"),
                artist: None,
                bpm: Some(128.0 + i as f32),
                tonart: audio_core::Tonart::parse("Am"),
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
