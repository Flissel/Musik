//! Begrenzer auf der Summe.
//!
//! Vier Kanäle addieren sich, und irgendwann übersteuert das. Ohne Begrenzer
//! wäre die Folge digitales Clipping — die hässlichste Art, laut zu werden.
//!
//! Bewusst einfach gehalten: Die Verstärkung fällt sofort und steigt langsam
//! wieder. Ohne Vorausschau (Lookahead) heißt das, dass sehr steile Transienten
//! verzerren statt weich begrenzt zu werden. Für die Summe eines DJ-Mixes ist
//! das vertretbar; ein Mastering-Begrenzer ist es nicht.

/// Voreinstellung: knapp unter Vollaussteuerung, damit noch Luft bleibt.
pub const DEFAULT_CEILING: f32 = 0.98;

#[derive(Debug, Clone)]
pub struct Limiter {
    ceiling: f32,
    gain: f32,
    release: f32,
}

impl Limiter {
    pub fn new(sample_rate: f32) -> Self {
        Limiter {
            ceiling: DEFAULT_CEILING,
            gain: 1.0,
            release: release_coefficient(0.100, sample_rate),
        }
    }

    pub fn set_ceiling(&mut self, ceiling: f32) {
        self.ceiling = ceiling.clamp(0.01, 1.0);
    }

    pub fn ceiling(&self) -> f32 {
        self.ceiling
    }

    /// Aktuelle Pegelreduktion in Dezibel, negativ oder null.
    pub fn reduction_db(&self) -> f32 {
        20.0 * self.gain.max(1e-6).log10()
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
    }

    /// Verarbeitet interleaved Stereo an Ort und Stelle.
    pub fn process(&mut self, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            let peak = frame[0].abs().max(frame[1].abs());

            let needed = if peak > 0.0 {
                (self.ceiling / peak).min(1.0)
            } else {
                1.0
            };

            if needed < self.gain {
                // Sofort greifen — hier darf nichts durchrutschen.
                self.gain = needed;
            } else {
                self.gain += (needed - self.gain) * self.release;
            }

            for sample in frame.iter_mut() {
                *sample = (*sample * self.gain).clamp(-self.ceiling, self.ceiling);
            }
        }
    }
}

fn release_coefficient(seconds: f32, sample_rate: f32) -> f32 {
    let samples = (seconds * sample_rate).max(1.0);
    1.0 - (-1.0 / samples).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{peak, rms, sine_stereo};

    const RATE: u32 = 48_000;

    #[test]
    fn lautes_material_bleibt_unter_der_decke() {
        let mut signal: Vec<f32> = sine_stereo(220.0, RATE, 0.5)
            .into_iter()
            .map(|v| v * 4.0)
            .collect();

        let mut limiter = Limiter::new(RATE as f32);
        limiter.process(&mut signal);

        let gemessen = peak(&signal);
        assert!(
            gemessen <= limiter.ceiling() + 1e-6,
            "Spitze {gemessen:.4} über der Decke {:.4}",
            limiter.ceiling()
        );
    }

    #[test]
    fn leises_material_bleibt_unangetastet() {
        let original = sine_stereo(220.0, RATE, 0.3)
            .into_iter()
            .map(|v| v * 0.2)
            .collect::<Vec<_>>();

        let mut signal = original.clone();
        let mut limiter = Limiter::new(RATE as f32);
        limiter.process(&mut signal);

        for (a, b) in original.iter().zip(&signal) {
            assert!((a - b).abs() < 1e-6, "leises Signal wurde verändert");
        }
    }

    #[test]
    fn ein_einzelner_ausreisser_reisst_nicht_alles_mit() {
        // Nach einem kurzen Peak muss der Pegel wieder hochkommen, sonst
        // duckt ein einziger Knall den ganzen Mix.
        let mut signal = sine_stereo(220.0, RATE, 1.0)
            .into_iter()
            .map(|v| v * 0.3)
            .collect::<Vec<_>>();
        signal[100] = 8.0;
        signal[101] = 8.0;

        let mut limiter = Limiter::new(RATE as f32);
        limiter.process(&mut signal);

        let spaet = rms(&signal[signal.len() * 3 / 4..]);
        let erwartet = 0.3 / 2.0_f32.sqrt();
        assert!(
            (spaet - erwartet).abs() < 0.02,
            "Pegel erholt sich nicht: {spaet:.3} statt {erwartet:.3}"
        );
    }

    #[test]
    fn decke_wird_auf_sinnvolle_werte_geklemmt() {
        let mut limiter = Limiter::new(RATE as f32);
        limiter.set_ceiling(5.0);
        assert_eq!(limiter.ceiling(), 1.0);
        limiter.set_ceiling(-1.0);
        assert_eq!(limiter.ceiling(), 0.01);
    }

    #[test]
    fn reduktion_wird_gemeldet() {
        let mut signal = sine_stereo(220.0, RATE, 0.2)
            .into_iter()
            .map(|v| v * 4.0)
            .collect::<Vec<_>>();

        let mut limiter = Limiter::new(RATE as f32);
        assert_eq!(limiter.reduction_db(), 0.0);

        limiter.process(&mut signal);
        assert!(
            limiter.reduction_db() < -6.0,
            "Reduktion nur {:.1} dB bei vierfachem Pegel",
            limiter.reduction_db()
        );
    }
}
