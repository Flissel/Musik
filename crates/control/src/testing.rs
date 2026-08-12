//! Ein vollständig verkabeltes Pult für Tests.
//!
//! Mit echter Engine und echter Kommandoschlange, nur ohne Audiogerät. Der
//! `EngineRunner` muss dabei am Leben bleiben: Fällt er weg, bricht die
//! Schlange, und jedes gesendete Kommando liefe ins Nichts — die Tests würden
//! grün, obwohl nichts ankommt.

use std::sync::Arc;

use audio_core::deck::DeckState;
use audio_engine::{Assign, DeckSource, Engine, EngineRunner, SilentSource};

use crate::pult::{DeckEintrag, KanalSpiegel, Steuerpult};

pub const RATE: u32 = 48_000;

/// Zwei Decks auf Kanal 1 und 2, dazu ein AUX-Kanal auf Thru.
pub fn pult_mit_zwei_decks() -> (Steuerpult, EngineRunner) {
    let mut engine = Engine::new(RATE as f32);

    let mut kanaele = Vec::new();
    for (name, assign) in [("DECK A", Assign::A), ("DECK B", Assign::B)] {
        let kanal = engine.add_channel(name, Box::new(SilentSource));
        engine.channel(kanal).set_assign(assign);
        kanaele.push((name, kanal, assign));
    }
    let aux = engine.add_channel("AUX", Box::new(SilentSource));
    engine.channel(aux).set_assign(Assign::Thru);

    let (handle, runner) = audio_engine::engine_channel(engine, 256);
    let mut pult = Steuerpult::neu(handle);

    for (name, kanal, assign) in &kanaele {
        pult.kanal_hinzufuegen(KanalSpiegel::neu(*name, *assign));
        pult.deck_hinzufuegen(DeckEintrag {
            state: Arc::new(DeckState::new()),
            kanal: *kanal,
            sample_rate: RATE,
            // Eine Minute, damit Positionen und Hot Cues Platz haben.
            frames: RATE as u64 * 60,
            titel: format!("Testtrack {name}"),
            artist: "Test".into(),
        });
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
    (pult, runner)
}
