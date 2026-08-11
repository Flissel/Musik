//! Sync: zwei Decks auf Tempo *und* Phase bringen.
//!
//! Der Teil, den man leicht halbiert. Gleiches Tempo genügt nicht — zwei Decks
//! mit exakt 128 BPM, deren Beats aber um eine Achtel versetzt liegen, klingen
//! genauso falsch wie zwei mit verschiedenem Tempo. Sync ist deshalb immer
//! zweierlei:
//!
//! 1. **Tempo** — der Pitchfader des Slaves wird so gesetzt, dass sein
//!    effektives Tempo dem des Masters entspricht.
//! 2. **Phase** — der Slave springt auf die Beat-Lage des Masters.
//!
//! Danach halten beide von selbst zusammen: Ist das effektive Tempo gleich,
//! läuft auch die Phase gleich schnell weiter. Es braucht keine fortlaufende
//! Nachregelung, solange niemand am Pitchfader dreht.
//!
//! ## Warum gesprungen und nicht geschoben wird
//!
//! Ein Sprung ist hörbar, wenn er groß ist. Weichere Verfahren ziehen das Tempo
//! kurz an, bis die Phase stimmt. Das ist angenehmer, aber auch träger, und für
//! den ersten Aufbau ist der Sprung ehrlicher: Er stimmt sofort und exakt.
//! Der Sprung nimmt immer den kürzeren Weg, höchstens eine halbe Beat-Länge.

use audio_core::deck::DeckState;
use audio_core::grid::shortest_phase_delta;

/// Was am Slave-Deck zu tun ist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyncPlan {
    /// Neuer Wert für den Pitchfader des Slaves.
    pub tempo: f32,
    /// Zielposition in Quell-Frames, wenn die Phase korrigiert werden muss.
    pub seek_to: Option<u64>,
    /// Wie weit die Phase danebenlag, in Beats (−0.5 … 0.5).
    pub phase_error_beats: f64,
}

/// Berechnet den Plan, ohne etwas zu verändern.
///
/// `None`, wenn einem der beiden Decks das Beatgrid fehlt — ohne Grid gibt es
/// weder Tempo noch Phase, auf die man synchronisieren könnte.
pub fn plan(master: &DeckState, slave: &DeckState, sample_rate: u32) -> Option<SyncPlan> {
    let master_grid = master.grid()?;
    let slave_grid = slave.grid()?;

    let master_bpm = master_grid.bpm * master.tempo();
    let tempo = master_bpm / slave_grid.bpm;
    if !tempo.is_finite() || tempo <= 0.0 {
        return None;
    }

    let master_pos = master.position_frames() as f64;
    let slave_pos = slave.position_frames() as f64;

    let master_phase = master_grid.phase_at(master_pos, sample_rate);
    let slave_phase = slave_grid.phase_at(slave_pos, sample_rate);
    let delta = shortest_phase_delta(slave_phase, master_phase);

    let ziel = slave_pos + delta * slave_grid.frames_per_beat(sample_rate);
    let seek_to = (ziel >= 0.0).then_some(ziel.round() as u64);

    Some(SyncPlan {
        tempo,
        seek_to,
        phase_error_beats: delta,
    })
}

/// Berechnet und wendet an. Gibt zurück, was getan wurde.
pub fn sync(master: &DeckState, slave: &DeckState, sample_rate: u32) -> Option<SyncPlan> {
    let plan = plan(master, slave, sample_rate)?;

    slave.set_tempo(plan.tempo);
    if let Some(frame) = plan.seek_to {
        slave.seek_frames(frame);
    }

    Some(plan)
}

/// Nur das Tempo angleichen, ohne Sprung.
///
/// Für den Fall, dass man selbst einschieben will — das Tempo abzunehmen ist
/// die mühsame Arbeit, die Phase trifft man von Hand.
pub fn sync_tempo_only(master: &DeckState, slave: &DeckState) -> Option<f32> {
    let master_bpm = master.effective_bpm()?;
    let slave_grid = slave.grid()?;

    let tempo = master_bpm / slave_grid.bpm;
    if !tempo.is_finite() || tempo <= 0.0 {
        return None;
    }

    slave.set_tempo(tempo);
    Some(tempo)
}

/// Aktueller Phasenversatz in Beats, −0.5 bis 0.5. Für Anzeigen und Tests.
pub fn phase_error(master: &DeckState, slave: &DeckState, sample_rate: u32) -> Option<f64> {
    let master_phase = master.beat_phase(sample_rate)?;
    let slave_phase = slave.beat_phase(sample_rate)?;
    Some(shortest_phase_delta(slave_phase, master_phase))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use audio_core::deck::Voice;
    use audio_core::{Beatgrid, Track};

    use super::*;
    use crate::testing::sine_stereo;

    const RATE: u32 = 48_000;

    fn deck(bpm: f32, anchor: u64, secs: f32) -> (Voice, Arc<DeckState>) {
        let track = Arc::new(Track {
            samples: sine_stereo(220.0, RATE, secs),
            sample_rate: RATE,
        });
        let state = Arc::new(DeckState::new());
        state.set_grid(Some(Beatgrid::new(bpm, anchor, 1.0)));
        state.set_playing(true);
        (Voice::new(track, Arc::clone(&state)), state)
    }

    #[test]
    fn tempo_wird_angeglichen() {
        let (_a, master) = deck(128.0, 0, 5.0);
        let (_b, slave) = deck(124.0, 0, 5.0);

        let plan = sync(&master, &slave, RATE).expect("kein Plan");

        assert!(
            (plan.tempo - 128.0 / 124.0).abs() < 1e-6,
            "Tempofaktor {:.5}",
            plan.tempo
        );
        assert!(
            (slave.effective_bpm().unwrap() - 128.0).abs() < 0.01,
            "effektives Tempo {:?}",
            slave.effective_bpm()
        );
    }

    #[test]
    fn der_pitchfader_des_masters_zaehlt_mit() {
        let (_a, master) = deck(128.0, 0, 5.0);
        let (_b, slave) = deck(128.0, 0, 5.0);

        master.set_tempo(1.05);
        sync(&master, &slave, RATE);

        assert!(
            (slave.tempo() - 1.05).abs() < 1e-5,
            "Slave folgt dem Pitchfader nicht: {:.4}",
            slave.tempo()
        );
    }

    #[test]
    fn die_phase_wird_korrigiert() {
        let (mut a, master) = deck(120.0, 0, 30.0);
        let (mut b, slave) = deck(120.0, 0, 30.0);

        // Slave um eine Viertel-Beat-Länge versetzen.
        let viertel = (60.0 / 120.0 * RATE as f64 * 0.25) as u64;
        slave.seek_frames(viertel);

        let mut out = vec![0.0; 1_024 * 2];
        a.render_stereo(&mut out);
        b.render_stereo(&mut out);

        let plan = sync(&master, &slave, RATE).expect("kein Plan");
        assert!(plan.seek_to.is_some(), "kein Sprung geplant");

        b.render_stereo(&mut out);
        a.render_stereo(&mut out);

        let rest = phase_error(&master, &slave, RATE).unwrap().abs();
        assert!(
            rest < 0.01,
            "nach Sync noch {rest:.4} Beats daneben (vorher {:.4})",
            plan.phase_error_beats
        );
    }

    #[test]
    fn der_sprung_nimmt_den_kurzen_weg() {
        let (_a, master) = deck(120.0, 0, 30.0);
        let (mut b, slave) = deck(120.0, 0, 30.0);

        // Slave liegt bei Phase 0.9, Master bei 0.0 — vorwärts sind es 0.1,
        // rückwärts 0.9. Der Plan muss den kurzen Weg nehmen.
        let fast_ganz = (60.0 / 120.0 * RATE as f64 * 0.9) as u64;
        slave.seek_frames(fast_ganz);

        // `seek_frames` ist nur eine Anforderung; ausgeführt wird sie im
        // Audio-Thread. Ohne einen Renderdurchlauf steht die Position noch
        // auf dem alten Wert und der Plan liefe ins Leere.
        let mut out = vec![0.0; 256 * 2];
        b.render_stereo(&mut out);

        let plan = plan(&master, &slave, RATE).expect("kein Plan");
        assert!(
            plan.phase_error_beats.abs() <= 0.5,
            "Sprung über eine halbe Beat-Länge: {:.3}",
            plan.phase_error_beats
        );
        assert!(
            plan.phase_error_beats > 0.0,
            "Sprung geht in die falsche Richtung: {:.3}",
            plan.phase_error_beats
        );
    }

    #[test]
    fn zwei_decks_bleiben_ueber_minuten_im_takt() {
        // Das Abnahmekriterium aus docs/PLAN.md, Phase 4.
        let (mut a, master) = deck(128.0, 0, 400.0);
        let (mut b, slave) = deck(124.0, 7_000, 400.0);

        sync(&master, &slave, RATE);

        let block = 4_096;
        let mut out_a = vec![0.0; block * 2];
        let mut out_b = vec![0.0; block * 2];

        let bloecke = (300.0 * RATE as f64 / block as f64) as usize;
        for _ in 0..bloecke {
            a.render_stereo(&mut out_a);
            b.render_stereo(&mut out_b);
        }

        let fehler = phase_error(&master, &slave, RATE).unwrap().abs();
        let ms = fehler * 60.0 / 128.0 * 1_000.0;

        assert!(
            ms < 5.0,
            "nach 5 Minuten {ms:.2} ms auseinander ({fehler:.5} Beats)"
        );
    }

    #[test]
    fn ohne_grid_gibt_es_keinen_sync() {
        let (_a, master) = deck(128.0, 0, 5.0);
        let (_b, slave) = deck(124.0, 0, 5.0);

        slave.set_grid(None);
        assert!(plan(&master, &slave, RATE).is_none());
        assert!(sync(&master, &slave, RATE).is_none());
        assert!(sync_tempo_only(&master, &slave).is_none());
        assert!(phase_error(&master, &slave, RATE).is_none());
    }

    #[test]
    fn nur_tempo_springt_nicht() {
        let (_a, master) = deck(128.0, 0, 30.0);
        let (_b, slave) = deck(124.0, 0, 30.0);

        slave.seek_frames(12_345);
        let vorher = slave.position_frames();

        let tempo = sync_tempo_only(&master, &slave).expect("kein Tempo");

        assert!((tempo - 128.0 / 124.0).abs() < 1e-6);
        assert_eq!(
            slave.position_frames(),
            vorher,
            "sync_tempo_only hat die Position verändert"
        );
    }
}
