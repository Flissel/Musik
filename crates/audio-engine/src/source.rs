//! Signalquellen eines Kanals.
//!
//! Der Mixer kennt keine Decks. Er kennt Quellen, die interleaved Stereo
//! liefern — ob dahinter ein Deck steht, ein Mikrofon am AUX-Eingang oder
//! später ein generierter Track, ist ihm gleich.
//!
//! Das ist dieselbe Trennung wie in `docs/ARCHITEKTUR.md`: austauschbare
//! Trackquellen, damit die Generierung sich später einhängen kann, ohne dass
//! am Mixer etwas anzufassen ist.

use audio_core::deck::Voice;

/// Liefert interleaved Stereo. Wird im Audio-Callback aufgerufen und darf
/// deshalb nicht allokieren, blockieren oder Speicher freigeben.
pub trait Source: Send {
    fn render(&mut self, out: &mut [f32]);
}

/// Ein Deck aus `audio-core`.
pub struct DeckSource {
    voice: Voice,
}

impl DeckSource {
    pub fn new(voice: Voice) -> Self {
        DeckSource { voice }
    }
}

impl Source for DeckSource {
    fn render(&mut self, out: &mut [f32]) {
        self.voice.render_stereo(out);
    }
}

/// Stille. Nützlich als Platzhalter für einen Kanal ohne geladenen Track.
pub struct SilentSource;

impl Source for SilentSource {
    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
    }
}

/// Spielt einen festen Puffer in Schleife. Für Testaufbauten und Zuspieler.
pub struct LoopSource {
    samples: Vec<f32>,
    position: usize,
}

impl LoopSource {
    /// `samples` ist interleaved Stereo.
    pub fn new(samples: Vec<f32>) -> Self {
        LoopSource {
            samples,
            position: 0,
        }
    }
}

impl Source for LoopSource {
    fn render(&mut self, out: &mut [f32]) {
        if self.samples.is_empty() {
            out.fill(0.0);
            return;
        }
        for sample in out.iter_mut() {
            *sample = self.samples[self.position];
            self.position = (self.position + 1) % self.samples.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stille_ist_still() {
        let mut out = vec![1.0; 64];
        SilentSource.render(&mut out);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn schleife_wiederholt_sich() {
        let mut source = LoopSource::new(vec![1.0, 2.0, 3.0, 4.0]);
        let mut out = vec![0.0; 10];
        source.render(&mut out);

        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0, 1.0, 2.0]);
    }

    #[test]
    fn leere_schleife_liefert_stille_statt_zu_paniken() {
        let mut source = LoopSource::new(Vec::new());
        let mut out = vec![1.0; 8];
        source.render(&mut out);
        assert!(out.iter().all(|v| *v == 0.0));
    }
}
