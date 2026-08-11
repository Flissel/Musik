//! Deck-Zustand und Rendering.
//!
//! Die Steuerung läuft ausschließlich über Atomics: Der Audio-Callback liest
//! sie, der Rest der Anwendung schreibt sie. Damit braucht der Abspielpfad kein
//! Lock, keine Allokation und keine Zuteilung von Speicher — die drei Dinge,
//! die in einem Audio-Callback zu Aussetzern führen.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use crate::stretch::Wsola;
use crate::track::{CHANNELS, Track};

/// Unterhalb dieser Abweichung von 1.0 wird direkt kopiert statt gestreckt.
const TEMPO_EPS: f64 = 1e-4;

/// Maximale Blockgröße, die am Stück gerendert wird.
const SCRATCH_FRAMES: usize = 4096;

pub struct DeckState {
    playing: AtomicBool,
    keylock: AtomicBool,
    tempo_bits: AtomicU32,
    position: AtomicU64,
    seek_to: AtomicI64,
    finished: AtomicBool,
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
        }
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

        let alive = if want_stretch {
            self.wsola
                .render(&self.track.samples, tempo, &mut self.pos, out)
        } else {
            varispeed(&self.track.samples, tempo, &mut self.pos, out)
        };

        if !alive {
            self.state.playing.store(false, Ordering::Relaxed);
            self.state.finished.store(true, Ordering::Relaxed);
        }

        let frames_total = self.track.frames() as f64;
        let clamped = self.pos.clamp(0.0, frames_total);
        self.state.position.store(clamped as u64, Ordering::Relaxed);
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

    #[test]
    fn play_setzt_das_ende_flag_zurueck() {
        let state = DeckState::new();
        state.finished.store(true, Ordering::Relaxed);

        state.set_playing(true);

        assert!(!state.is_finished());
        assert!(state.is_playing());
    }
}
