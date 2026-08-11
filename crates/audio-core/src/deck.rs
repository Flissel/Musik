//! Deck-Zustand und Rendering.
//!
//! Die Steuerung läuft ausschließlich über Atomics: Der Audio-Callback liest
//! sie, der Rest der Anwendung schreibt sie. Damit braucht der Abspielpfad kein
//! Lock, keine Allokation und keine Zuteilung von Speicher — die drei Dinge,
//! die in einem Audio-Callback zu Aussetzern führen.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use crate::grid::Beatgrid;
use crate::stretch::Wsola;
use crate::track::{CHANNELS, Track};

/// Unterhalb dieser Abweichung von 1.0 wird direkt kopiert statt gestreckt.
const TEMPO_EPS: f64 = 1e-4;

/// Maximale Blockgröße, die am Stück gerendert wird.
const SCRATCH_FRAMES: usize = 4096;

/// Anzahl der Hot Cues je Deck, wie bei Traktor.
pub const HOT_CUES: usize = 8;

pub struct DeckState {
    playing: AtomicBool,
    keylock: AtomicBool,
    tempo_bits: AtomicU32,
    position: AtomicU64,
    seek_to: AtomicI64,
    finished: AtomicBool,

    /// Beatgrid als Atomics statt als Struktur hinter einem Lock — der
    /// Audio-Thread liest es, und Locks haben dort nichts zu suchen.
    /// BPM 0.0 bedeutet: kein Grid bekannt.
    grid_bpm_bits: AtomicU32,
    grid_anchor: AtomicU64,

    /// −1 = nicht gesetzt.
    cues: [AtomicI64; HOT_CUES],

    loop_start: AtomicI64,
    loop_end: AtomicI64,
    loop_active: AtomicBool,
}

impl Default for DeckState {
    fn default() -> Self {
        Self::new()
    }
}

impl DeckState {
    pub fn new() -> Self {
        DeckState {
            playing: AtomicBool::new(false),
            keylock: AtomicBool::new(true),
            tempo_bits: AtomicU32::new(1.0f32.to_bits()),
            position: AtomicU64::new(0),
            seek_to: AtomicI64::new(-1),
            finished: AtomicBool::new(false),
            grid_bpm_bits: AtomicU32::new(0.0f32.to_bits()),
            grid_anchor: AtomicU64::new(0),
            cues: std::array::from_fn(|_| AtomicI64::new(-1)),
            loop_start: AtomicI64::new(-1),
            loop_end: AtomicI64::new(-1),
            loop_active: AtomicBool::new(false),
        }
    }

    /// Hinterlegt das Beatgrid aus der Analyse. `None` löscht es.
    pub fn set_grid(&self, grid: Option<Beatgrid>) {
        match grid.filter(|g| g.is_usable()) {
            Some(g) => {
                self.grid_anchor.store(g.anchor_frames, Ordering::Relaxed);
                self.grid_bpm_bits.store(g.bpm.to_bits(), Ordering::Relaxed);
            }
            None => self
                .grid_bpm_bits
                .store(0.0f32.to_bits(), Ordering::Relaxed),
        }
    }

    pub fn grid(&self) -> Option<Beatgrid> {
        let bpm = f32::from_bits(self.grid_bpm_bits.load(Ordering::Relaxed));
        let grid = Beatgrid::new(bpm, self.grid_anchor.load(Ordering::Relaxed), 1.0);
        grid.is_usable().then_some(grid)
    }

    /// Effektives Tempo in BPM — Grid-Tempo mal Pitchfader.
    pub fn effective_bpm(&self) -> Option<f32> {
        self.grid().map(|g| g.bpm * self.tempo())
    }

    /// Lage im laufenden Beat, 0.0 bis 1.0.
    pub fn beat_phase(&self, sample_rate: u32) -> Option<f64> {
        self.grid()
            .map(|g| g.phase_at(self.position_frames() as f64, sample_rate))
    }

    /// Setzt einen Hot Cue auf die angegebene Position.
    pub fn set_cue(&self, index: usize, frame: Option<u64>) {
        if let Some(slot) = self.cues.get(index) {
            slot.store(frame.map(|f| f as i64).unwrap_or(-1), Ordering::Relaxed);
        }
    }

    pub fn cue(&self, index: usize) -> Option<u64> {
        let raw = self.cues.get(index)?.load(Ordering::Relaxed);
        (raw >= 0).then_some(raw as u64)
    }

    /// Springt auf einen Hot Cue. Gibt zurück, ob er gesetzt war.
    pub fn jump_to_cue(&self, index: usize) -> bool {
        match self.cue(index) {
            Some(frame) => {
                self.seek_frames(frame);
                true
            }
            None => false,
        }
    }

    /// Setzt eine Schleife. `end` muss hinter `start` liegen.
    pub fn set_loop(&self, start: u64, end: u64) -> bool {
        if end <= start {
            return false;
        }
        self.loop_start.store(start as i64, Ordering::Relaxed);
        self.loop_end.store(end as i64, Ordering::Relaxed);
        true
    }

    pub fn loop_range(&self) -> Option<(u64, u64)> {
        let start = self.loop_start.load(Ordering::Relaxed);
        let end = self.loop_end.load(Ordering::Relaxed);
        (start >= 0 && end > start).then_some((start as u64, end as u64))
    }

    /// Schaltet die Schleife scharf. Ohne gesetzten Bereich passiert nichts.
    pub fn set_loop_active(&self, active: bool) -> bool {
        let ok = !active || self.loop_range().is_some();
        self.loop_active.store(active && ok, Ordering::Relaxed);
        ok
    }

    pub fn is_looping(&self) -> bool {
        self.loop_active.load(Ordering::Relaxed) && self.loop_range().is_some()
    }

    /// Legt eine Schleife über `beats` Beats ab der aktuellen Position,
    /// eingerastet auf das Grid.
    ///
    /// Ohne Einrasten säße der Anfang irgendwo im Takt, und die Schleife
    /// klänge bei jedem Durchlauf wie ein Stolperer.
    pub fn set_loop_beats(&self, beats: f64, sample_rate: u32) -> bool {
        let Some(grid) = self.grid() else {
            return false;
        };
        if beats <= 0.0 {
            return false;
        }

        let start = grid
            .snap(self.position_frames() as f64, sample_rate)
            .max(0.0);
        let length = beats * grid.frames_per_beat(sample_rate);

        self.set_loop(start as u64, (start + length) as u64)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn set_playing(&self, on: bool) {
        if on {
            self.finished.store(false, Ordering::Relaxed);
        }
        self.playing.store(on, Ordering::Relaxed);
    }

    pub fn toggle_playing(&self) -> bool {
        let next = !self.is_playing();
        self.set_playing(next);
        next
    }

    pub fn keylock(&self) -> bool {
        self.keylock.load(Ordering::Relaxed)
    }

    pub fn set_keylock(&self, on: bool) {
        self.keylock.store(on, Ordering::Relaxed);
    }

    /// Tempoverhältnis: 1.0 = Originalgeschwindigkeit, 1.06 = +6 %.
    pub fn tempo(&self) -> f32 {
        f32::from_bits(self.tempo_bits.load(Ordering::Relaxed))
    }

    pub fn set_tempo(&self, ratio: f32) {
        let clamped = ratio.clamp(0.25, 4.0);
        self.tempo_bits.store(clamped.to_bits(), Ordering::Relaxed);
    }

    pub fn position_frames(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    pub fn seek_frames(&self, frame: u64) {
        self.seek_to.store(frame as i64, Ordering::Relaxed);
    }

    /// Track ist bis zum Ende gelaufen.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

/// Die Audio-Thread-Seite eines Decks. Wird vom Callback exklusiv besessen.
pub struct Voice {
    track: Arc<Track>,
    state: Arc<DeckState>,
    wsola: Wsola,
    scratch: Vec<f32>,
    pos: f64,
    stretching: bool,
}

impl Voice {
    pub fn new(track: Arc<Track>, state: Arc<DeckState>) -> Self {
        Voice {
            track,
            state,
            wsola: Wsola::new(),
            scratch: vec![0.0; SCRATCH_FRAMES * CHANNELS],
            pos: 0.0,
            stretching: false,
        }
    }

    /// Schreibt einen Block in den Gerätepuffer. `out_channels` ist die
    /// Kanalzahl des Geräts, nicht die des Tracks.
    pub fn render(&mut self, out: &mut [f32], out_channels: usize) {
        if out_channels == 0 {
            return;
        }
        let total_frames = out.len() / out_channels;
        let mut done = 0;

        while done < total_frames {
            let n = (total_frames - done).min(SCRATCH_FRAMES);
            let len = n * CHANNELS;
            let mut scratch = std::mem::take(&mut self.scratch);
            self.render_stereo(&mut scratch[..len]);
            self.scratch = scratch;

            for i in 0..n {
                let l = self.scratch[i * CHANNELS];
                let r = self.scratch[i * CHANNELS + 1];
                let base = (done + i) * out_channels;
                let frame = &mut out[base..base + out_channels];
                if out_channels == 1 {
                    frame[0] = 0.5 * (l + r);
                } else {
                    frame[0] = l;
                    frame[1] = r;
                    frame[2..].fill(0.0);
                }
            }

            done += n;
        }
    }

    /// Schreibt interleaved Stereo in `out`.
    ///
    /// Das ist der Einstieg für den Mixer: Ein Deck ist dort eine Quelle unter
    /// mehreren und liefert immer Stereo, unabhängig davon, wie das
    /// Ausgabegerät aussieht.
    pub fn render_stereo(&mut self, out: &mut [f32]) {
        if let Some(target) = self.take_seek() {
            self.pos = target;
            self.wsola.reset();
        }

        if !self.state.is_playing() {
            out.fill(0.0);
            return;
        }

        let tempo = self.state.tempo() as f64;
        let want_stretch = self.state.keylock() && (tempo - 1.0).abs() > TEMPO_EPS;

        if want_stretch != self.stretching {
            self.wsola.reset();
            self.stretching = want_stretch;
        }

        let mut alive = true;
        let mut written = 0usize;

        // Blockweise bis zur nächsten Schleifengrenze rendern. Erst am Ende des
        // Callback-Blocks umzubrechen wäre bequemer, würde die Schleife aber um
        // bis zu einen Block verlängern — hörbar als ungleichmäßiger Takt.
        while written < out.len() {
            let rest = &mut out[written..];
            let chunk = self.frames_until_loop_end(tempo).min(rest.len() / CHANNELS);
            let len = chunk.max(1) * CHANNELS;
            let len = len.min(rest.len());

            alive = if want_stretch {
                self.wsola
                    .render(&self.track.samples, tempo, &mut self.pos, &mut rest[..len])
            } else {
                varispeed(&self.track.samples, tempo, &mut self.pos, &mut rest[..len])
            };

            written += len;

            if !alive {
                break;
            }
            self.wrap_loop();
        }

        if !alive {
            self.state.playing.store(false, Ordering::Relaxed);
            self.state.finished.store(true, Ordering::Relaxed);
        }

        let frames_total = self.track.frames() as f64;
        let clamped = self.pos.clamp(0.0, frames_total);
        self.state.position.store(clamped as u64, Ordering::Relaxed);
    }

    /// Wie viele Ausgabe-Frames bis zum Schleifenende gerendert werden dürfen.
    /// Ohne aktive Schleife: unbegrenzt.
    fn frames_until_loop_end(&self, tempo: f64) -> usize {
        let Some((_, end)) = self.active_loop() else {
            return usize::MAX;
        };
        let verbleibend = end as f64 - self.pos;
        if verbleibend <= 0.0 {
            return 1;
        }
        // Quell-Frames in Ausgabe-Frames umrechnen: bei doppeltem Tempo
        // verbraucht ein Ausgabe-Frame zwei Quell-Frames.
        let schritt = tempo.max(1e-6);
        (verbleibend / schritt).ceil() as usize
    }

    /// Setzt die Position zurück, sobald das Schleifenende überschritten ist.
    fn wrap_loop(&mut self) {
        let Some((start, end)) = self.active_loop() else {
            return;
        };
        if self.pos < end as f64 {
            return;
        }

        let laenge = (end - start) as f64;
        let ueberschuss = (self.pos - end as f64).rem_euclid(laenge);
        self.pos = start as f64 + ueberschuss;
        self.wsola.reset();
    }

    fn active_loop(&self) -> Option<(u64, u64)> {
        self.state
            .is_looping()
            .then(|| self.state.loop_range())
            .flatten()
    }

    fn take_seek(&mut self) -> Option<f64> {
        let raw = self.state.seek_to.swap(-1, Ordering::Relaxed);
        if raw < 0 {
            return None;
        }
        let max = self.track.frames().saturating_sub(1) as i64;
        Some(raw.min(max) as f64)
    }
}

/// Abspielen mit veränderter Rate — Tempo *und* Tonhöhe wandern mit, wie beim
/// Pitchfader eines Plattenspielers. Das ist der Modus ohne Keylock.
fn varispeed(src: &[f32], ratio: f64, pos: &mut f64, out: &mut [f32]) -> bool {
    let frames = src.len() / CHANNELS;
    if frames < 2 {
        out.fill(0.0);
        return false;
    }

    let blocks = out.len() / CHANNELS;
    for i in 0..blocks {
        let idx = pos.floor();
        if idx < 0.0 || idx as usize + 1 >= frames {
            out[i * CHANNELS..].fill(0.0);
            return false;
        }
        let idx = idx as usize;
        let frac = (*pos - idx as f64) as f32;

        for c in 0..CHANNELS {
            let a = src[idx * CHANNELS + c];
            let b = src[(idx + 1) * CHANNELS + c];
            out[i * CHANNELS + c] = a + (b - a) * frac;
        }

        *pos += ratio;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dominant_freq, sine};

    const RATE: u32 = 44_100;

    /// Ohne Keylock wandert die Tonhöhe mit dem Tempo — wie beim Plattenspieler.
    #[test]
    fn varispeed_verschiebt_die_tonhoehe() {
        let src = sine(440.0, RATE, 10.0);
        let mut pos = 0.0;
        let mut out = vec![0.0; 4 * RATE as usize * CHANNELS];

        assert!(varispeed(&src, 1.06, &mut pos, &mut out));

        let freq = dominant_freq(&out, RATE);
        let erwartet = 440.0 * 1.06;
        assert!(
            (freq - erwartet).abs() < 6.0,
            "erwartet ~{erwartet:.1} Hz, gemessen {freq:.1} Hz"
        );
    }

    #[test]
    fn varispeed_meldet_das_ende() {
        let src = sine(440.0, RATE, 0.1);
        let mut pos = 0.0;
        let mut out = vec![0.5; RATE as usize * CHANNELS];

        assert!(!varispeed(&src, 1.0, &mut pos, &mut out));
        assert_eq!(out.last().copied(), Some(0.0));
    }

    #[test]
    fn tempo_wird_auf_sinnvolle_grenzen_geklemmt() {
        let state = DeckState::new();

        state.set_tempo(99.0);
        assert_eq!(state.tempo(), 4.0);

        state.set_tempo(0.0);
        assert_eq!(state.tempo(), 0.25);

        state.set_tempo(1.06);
        assert!((state.tempo() - 1.06).abs() < 1e-6);
    }

    fn deck_mit_ton(secs: f32) -> (Voice, Arc<DeckState>) {
        let track = Arc::new(Track {
            samples: sine(440.0, RATE, secs),
            sample_rate: RATE,
        });
        let state = Arc::new(DeckState::new());
        state.set_playing(true);
        (Voice::new(track, Arc::clone(&state)), state)
    }

    #[test]
    fn hot_cues_speichern_und_springen() {
        let (_voice, state) = deck_mit_ton(10.0);

        assert_eq!(state.cue(0), None);
        assert!(!state.jump_to_cue(0), "Sprung auf leeren Cue meldet Erfolg");

        state.set_cue(0, Some(12_345));
        assert_eq!(state.cue(0), Some(12_345));
        assert!(state.jump_to_cue(0));

        state.set_cue(0, None);
        assert_eq!(state.cue(0), None);
    }

    #[test]
    fn cue_index_ausserhalb_paniert_nicht() {
        let (_voice, state) = deck_mit_ton(1.0);
        state.set_cue(99, Some(1));
        assert_eq!(state.cue(99), None);
        assert!(!state.jump_to_cue(99));
    }

    #[test]
    fn schleife_haelt_die_position_im_bereich() {
        let (mut voice, state) = deck_mit_ton(10.0);
        let (start, end) = (RATE as u64, RATE as u64 + 12_000);

        assert!(state.set_loop(start, end));
        state.seek_frames(start);
        assert!(state.set_loop_active(true));

        // Deutlich mehr rendern, als die Schleife lang ist.
        let mut out = vec![0.0; 8_192 * CHANNELS];
        for _ in 0..12 {
            voice.render_stereo(&mut out);
            let pos = state.position_frames();
            assert!(
                (start..end).contains(&pos),
                "Position {pos} außerhalb der Schleife {start}..{end}"
            );
        }

        assert!(
            !state.is_finished(),
            "Schleife darf nicht ans Trackende laufen"
        );
    }

    #[test]
    fn schleife_aus_laesst_weiterlaufen() {
        let (mut voice, state) = deck_mit_ton(10.0);
        state.set_loop(RATE as u64, RATE as u64 + 12_000);
        state.seek_frames(RATE as u64);
        state.set_loop_active(false);

        let mut out = vec![0.0; 8_192 * CHANNELS];
        for _ in 0..6 {
            voice.render_stereo(&mut out);
        }

        assert!(
            state.position_frames() > RATE as u64 + 12_000,
            "ohne aktive Schleife bleibt das Deck stehen"
        );
    }

    #[test]
    fn unsinnige_schleifen_werden_abgelehnt() {
        let (_voice, state) = deck_mit_ton(1.0);

        assert!(!state.set_loop(500, 500), "leere Schleife akzeptiert");
        assert!(
            !state.set_loop(500, 100),
            "rückwärts laufende Schleife akzeptiert"
        );
        assert!(state.loop_range().is_none());

        assert!(
            !state.set_loop_active(true),
            "Schleife ohne Bereich scharf gestellt"
        );
        assert!(!state.is_looping());
    }

    #[test]
    fn beat_schleife_rastet_auf_das_grid() {
        let (_voice, state) = deck_mit_ton(10.0);
        // 120 BPM = 22 050 Frames je Beat bei 44,1 kHz.
        state.set_grid(Some(Beatgrid::new(120.0, 1_000, 1.0)));
        state.seek_frames(30_000);

        assert!(state.set_loop_beats(4.0, RATE));
        let (start, end) = state.loop_range().expect("keine Schleife gesetzt");

        // Start muss auf einem Beat sitzen, nicht auf 30 000.
        let grid = state.grid().unwrap();
        let phase = grid.phase_at(start as f64, RATE);
        assert!(
            !(1e-6..=1.0 - 1e-6).contains(&phase),
            "Schleifenanfang liegt bei Phase {phase:.4} statt auf dem Beat"
        );

        let beats = (end - start) as f64 / grid.frames_per_beat(RATE);
        assert!(
            (beats - 4.0).abs() < 1e-3,
            "Schleife ist {beats:.3} Beats lang"
        );
    }

    #[test]
    fn ohne_grid_gibt_es_keine_beat_schleife() {
        let (_voice, state) = deck_mit_ton(1.0);
        assert!(!state.set_loop_beats(4.0, RATE));
    }

    #[test]
    fn grid_ueberlebt_die_atomics() {
        let (_voice, state) = deck_mit_ton(1.0);
        assert!(state.grid().is_none());

        state.set_grid(Some(Beatgrid::new(128.5, 4_242, 0.8)));
        let grid = state.grid().expect("Grid verschwunden");
        assert!((grid.bpm - 128.5).abs() < 1e-4);
        assert_eq!(grid.anchor_frames, 4_242);

        state.set_grid(None);
        assert!(state.grid().is_none());

        // Unbrauchbare Grids werden gar nicht erst übernommen.
        state.set_grid(Some(Beatgrid::new(0.0, 1, 1.0)));
        assert!(state.grid().is_none());
    }

    #[test]
    fn effektives_tempo_beruecksichtigt_den_pitchfader() {
        let (_voice, state) = deck_mit_ton(1.0);
        assert_eq!(state.effective_bpm(), None);

        state.set_grid(Some(Beatgrid::new(120.0, 0, 1.0)));
        state.set_tempo(1.05);

        let bpm = state.effective_bpm().unwrap();
        assert!((bpm - 126.0).abs() < 0.01, "effektives Tempo {bpm:.2}");
    }

    #[test]
    fn play_setzt_das_ende_flag_zurueck() {
        let state = DeckState::new();
        state.finished.store(true, Ordering::Relaxed);

        state.set_playing(true);

        assert!(!state.is_finished());
        assert!(state.is_playing());
    }
}
