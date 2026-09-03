//! Beatgrid: die Umrechnung zwischen Sample-Position und Taktzählung.
//!
//! Ein Tempo allein reicht für Sync nicht. Zwei Decks mit identischem BPM,
//! deren Beats aber versetzt liegen, klingen genauso falsch wie zwei mit
//! verschiedenem Tempo. Gebraucht werden immer beide Größen: **Tempo** und
//! **Phase** — und die Phase kommt aus dem Anker.
//!
//! Alle Angaben beziehen sich auf *Quell-Frames*, nicht auf Ausgabezeit. Ein
//! Deck, das mit 1,03-fachem Tempo läuft, wandert schneller durch das Grid,
//! aber das Grid selbst bleibt, wie es beim Analysieren gemessen wurde.

/// Tempo und Lage der Beats eines Tracks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beatgrid {
    pub bpm: f32,
    /// Position eines Beats in Sample-Frames. Welcher Beat, ist gleichgültig —
    /// das Raster wiederholt sich.
    pub anchor_frames: u64,
    /// 0..1, wie verlässlich die Erkennung war.
    pub confidence: f32,
}

impl Beatgrid {
    pub fn new(bpm: f32, anchor_frames: u64, confidence: f32) -> Self {
        Beatgrid {
            bpm,
            anchor_frames,
            confidence,
        }
    }

    pub fn is_usable(&self) -> bool {
        self.bpm.is_finite() && self.bpm > 1.0
    }

    /// Länge eines Beats in Sample-Frames.
    pub fn frames_per_beat(&self, sample_rate: u32) -> f64 {
        60.0 / self.bpm as f64 * sample_rate as f64
    }

    /// Fortlaufende Beat-Nummer an einer Position. Kann negativ sein, wenn die
    /// Position vor dem Anker liegt.
    pub fn beat_at(&self, frame: f64, sample_rate: u32) -> f64 {
        (frame - self.anchor_frames as f64) / self.frames_per_beat(sample_rate)
    }

    /// Umkehrung: Position einer Beat-Nummer.
    pub fn frame_of_beat(&self, beat: f64, sample_rate: u32) -> f64 {
        self.anchor_frames as f64 + beat * self.frames_per_beat(sample_rate)
    }

    /// Lage innerhalb des laufenden Beats, 0.0 bis 1.0.
    pub fn phase_at(&self, frame: f64, sample_rate: u32) -> f64 {
        self.beat_at(frame, sample_rate).rem_euclid(1.0)
    }

    /// Nächstgelegene Beat-Position zu `frame`.
    pub fn snap(&self, frame: f64, sample_rate: u32) -> f64 {
        let beat = self.beat_at(frame, sample_rate).round();
        self.frame_of_beat(beat, sample_rate)
    }
}

/// Kürzester Weg von `from` nach `to` auf einem Kreis der Länge 1.
///
/// Liegt ein Deck bei Phase 0,9 und das andere bei 0,1, ist der richtige Weg
/// +0,2 vorwärts und nicht −0,8 rückwärts. Ohne diese Wicklung würde Sync
/// einen fast ganzen Beat springen, wo ein Zehntel genügt.
pub fn shortest_phase_delta(from: f64, to: f64) -> f64 {
    let raw = (to - from).rem_euclid(1.0);
    if raw > 0.5 { raw - 1.0 } else { raw }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn grid() -> Beatgrid {
        // 120 BPM: ein Beat sind exakt 24 000 Frames bei 48 kHz.
        Beatgrid::new(120.0, 10_000, 1.0)
    }

    #[test]
    fn beat_laenge_stimmt() {
        assert!((grid().frames_per_beat(RATE) - 24_000.0).abs() < 1e-6);
    }

    #[test]
    fn anker_liegt_auf_beat_null() {
        let g = grid();
        assert!((g.beat_at(10_000.0, RATE) - 0.0).abs() < 1e-9);
        assert!((g.beat_at(34_000.0, RATE) - 1.0).abs() < 1e-9);
        assert!((g.beat_at(-14_000.0, RATE) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn hin_und_zurueck_ist_verlustfrei() {
        let g = grid();
        for beat in [-3.5, 0.0, 1.25, 17.75] {
            let frame = g.frame_of_beat(beat, RATE);
            assert!((g.beat_at(frame, RATE) - beat).abs() < 1e-9);
        }
    }

    #[test]
    fn phase_laeuft_von_null_bis_eins() {
        let g = grid();
        assert!((g.phase_at(10_000.0, RATE) - 0.0).abs() < 1e-9);
        assert!((g.phase_at(22_000.0, RATE) - 0.5).abs() < 1e-9);
        assert!((g.phase_at(33_999.0, RATE) - 0.99995).abs() < 1e-4);

        // Auch vor dem Anker muss die Phase im Bereich bleiben.
        let vorher = g.phase_at(-1_000.0, RATE);
        assert!((0.0..1.0).contains(&vorher), "Phase {vorher} außerhalb");
    }

    #[test]
    fn snap_zieht_auf_den_naechsten_beat() {
        let g = grid();
        assert!((g.snap(11_000.0, RATE) - 10_000.0).abs() < 1e-6);
        assert!((g.snap(33_000.0, RATE) - 34_000.0).abs() < 1e-6);
    }

    #[test]
    fn phasendifferenz_nimmt_den_kurzen_weg() {
        // 0.9 → 0.1 sind 0.2 vorwärts, nicht 0.8 rückwärts.
        assert!((shortest_phase_delta(0.9, 0.1) - 0.2).abs() < 1e-9);
        assert!((shortest_phase_delta(0.1, 0.9) + 0.2).abs() < 1e-9);
        assert!((shortest_phase_delta(0.25, 0.75) - 0.5).abs() < 1e-9);
        assert!(shortest_phase_delta(0.5, 0.5).abs() < 1e-9);
    }

    #[test]
    fn unbrauchbare_grids_werden_erkannt() {
        assert!(grid().is_usable());
        assert!(!Beatgrid::new(0.0, 0, 0.0).is_usable());
        assert!(!Beatgrid::new(f32::NAN, 0, 0.0).is_usable());
    }
}
