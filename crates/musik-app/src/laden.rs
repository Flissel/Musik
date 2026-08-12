//! Einen Track auf ein Deck legen.
//!
//! Dekodieren und Analysieren dauern und blockieren — deshalb passiert beides
//! hier, im Thread der Oberfläche, und nicht im Audio-Callback. Der bekommt nur
//! die fertige Quelle zugeschoben und reicht die alte zurück.

use std::sync::Arc;

use analysis::Store;
use anyhow::{Context, Result};
use audio_core::deck::Voice;
use audio_core::{Beatgrid, Track};
use audio_engine::DeckSource;
use library::TrackRecord;

use crate::app::MusikApp;

/// Lädt den Track eines Library-Eintrags auf ein Deck.
pub fn track_auf_deck(app: &mut MusikApp, deck: usize, eintrag: &TrackRecord) -> Result<()> {
    let rate = app
        .decks
        .get(deck)
        .context("Deck gibt es nicht")?
        .sample_rate;

    let track = Track::decode_file(std::path::Path::new(&eintrag.path))
        .with_context(|| format!("{} nicht lesbar", eintrag.path))?
        .resampled_to(rate);

    let store = Store::new(&app.analyse_cache);
    let (analyse, gerechnet) = analysis::analyze_cached(&track, &store);
    if gerechnet {
        let _ = store.save(&analyse);
    }

    let peaks = analyse
        .peaks
        .iter()
        .filter_map(|p| p.to_level())
        .collect::<Vec<_>>();

    let frames = track.frames() as u64;
    let state = Arc::clone(&app.decks[deck].state);
    let titel = crate::app::anzeigename(eintrag);
    let artist = eintrag.artist.clone().unwrap_or_default();

    // Der neue Track startet gestoppt am Anfang — ein Deck, das beim Laden
    // losläuft, ist ein Unfall.
    state.set_playing(false);
    state.seek_frames(0);
    state.set_loop_active(false);
    for i in 0..audio_core::deck::HOT_CUES {
        state.set_cue(i, None);
    }
    state.set_grid(
        analyse
            .bpm
            .map(|bpm| Beatgrid::new(bpm, analyse.beat_anchor_frames.unwrap_or(0), 1.0)),
    );

    let voice = Voice::new(Arc::new(track), Arc::clone(&state));

    {
        let mut pult = app
            .pult
            .lock()
            .map_err(|_| anyhow::anyhow!("Steuerpult ist in einem unklaren Zustand"))?;

        let kanal = pult
            .decks()
            .get(deck)
            .context("Deck gibt es im Steuerpult nicht")?
            .kanal;

        if !pult
            .handle_mut()
            .load(kanal, Box::new(DeckSource::new(voice)))
        {
            anyhow::bail!("die Ladeschlange ist voll — gleich noch einmal versuchen");
        }

        // Erst nach dem erfolgreichen Laden umbenennen: Sonst zeigte das Pult
        // einen Titel an, der gar nicht auf dem Deck liegt.
        if let Some(eintrag) = pult.deck_mut(deck) {
            eintrag.titel = titel;
            eintrag.artist = artist;
            eintrag.frames = frames;
        }
    }

    let ui = &mut app.decks[deck];
    ui.peaks = peaks;
    ui.frames = frames;

    Ok(())
}
