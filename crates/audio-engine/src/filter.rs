//! Filterknopf nach DJ-Art: ein Regler, zwei Richtungen.
//!
//! Mitte ist neutral, nach links fährt ein Tiefpass herunter, nach rechts ein
//! Hochpass herauf. Ein einziger Regler statt zweier ist keine Sparmaßnahme —
//! im Livebetrieb greift man blind danach, und die Mittelstellung muss sich
//! ertasten lassen.
//!
//! Die Frequenzen laufen exponentiell, weil Gehör so funktioniert: Von 100 auf
//! 200 Hz ist gefühlt derselbe Schritt wie von 1000 auf 2000.

use crate::svf::Svf;

/// Unterhalb dieser Auslenkung bleibt der Filter aus.
const DEADZONE: f32 = 0.02;

const LP_MAX_HZ: f32 = 20_000.0;
const LP_MIN_HZ: f32 = 40.0;
const HP_MIN_HZ: f32 = 20.0;
const HP_MAX_HZ: f32 = 12_000.0;

const Q: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bypass,
    LowPass,
    HighPass,
}

#[derive(Debug, Clone)]
pub struct DjFilter {
    svf: [Svf; 2],
    mode: Mode,
    position: f32,
    sample_rate: f32,
}

impl DjFilter {
    pub fn new(sample_rate: f32) -> Self {
        DjFilter {
            svf: [Svf::new(); 2],
            mode: Mode::Bypass,
            position: 0.0,
            sample_rate,
        }
    }

    /// −1 = Tiefpass ganz zu, 0 = neutral, +1 = Hochpass ganz auf.
    pub fn set_position(&mut self, position: f32) {
        let position = position.clamp(-1.0, 1.0);
        self.position = position;

        let (mode, cutoff) = if position < -DEADZONE {
            let t = (-position - DEADZONE) / (1.0 - DEADZONE);
            (Mode::LowPass, exp_sweep(LP_MAX_HZ, LP_MIN_HZ, t))
        } else if position > DEADZONE {
            let t = (position - DEADZONE) / (1.0 - DEADZONE);
            (Mode::HighPass, exp_sweep(HP_MIN_HZ, HP_MAX_HZ, t))
        } else {
            (Mode::Bypass, 1_000.0)
        };

        if mode != self.mode {
            // Beim Umschalten den Zustand verwerfen, sonst knackt der Übergang.
            for svf in self.svf.iter_mut() {
                svf.reset();
            }
            self.mode = mode;
        }

        for svf in self.svf.iter_mut() {
            svf.set(cutoff, Q, self.sample_rate);
        }
    }

    pub fn position(&self) -> f32 {
        self.position
    }

    pub fn is_active(&self) -> bool {
        self.mode != Mode::Bypass
    }

    pub fn reset(&mut self) {
        for svf in self.svf.iter_mut() {
            svf.reset();
        }
    }

    pub fn process(&mut self, buffer: &mut [f32]) {
        if self.mode == Mode::Bypass {
            return;
        }

        for frame in buffer.chunks_exact_mut(2) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let out = self.svf[ch].process(*sample);
                *sample = match self.mode {
                    Mode::LowPass => out.low,
                    Mode::HighPass => out.high,
                    Mode::Bypass => *sample,
                };
            }
        }
    }
}

fn exp_sweep(from: f32, to: f32, t: f32) -> f32 {
    from * (to / from).powf(t.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{rms, sine_stereo};

    const RATE: u32 = 48_000;

    fn verhaeltnis(freq: f32, position: f32) -> f32 {
        let signal = sine_stereo(freq, RATE, 0.5);
        let mut filter = DjFilter::new(RATE as f32);
        filter.set_position(position);

        let mut out = signal.clone();
        filter.process(&mut out);

        let half = out.len() / 2;
        rms(&out[half..]) / rms(&signal[half..])
    }

    #[test]
    fn mittelstellung_laesst_alles_durch() {
        for freq in [50.0, 1_000.0, 12_000.0] {
            let v = verhaeltnis(freq, 0.0);
            assert!((v - 1.0).abs() < 1e-4, "{freq} Hz verändert auf {v:.4}");
        }
    }

    #[test]
    fn die_totzone_gilt_in_beide_richtungen() {
        assert!(!DjFilter::new(48_000.0).is_active());

        let mut f = DjFilter::new(48_000.0);
        f.set_position(0.01);
        assert!(!f.is_active(), "winzige Auslenkung schaltet schon");

        f.set_position(-0.01);
        assert!(!f.is_active());

        f.set_position(0.5);
        assert!(f.is_active());
    }

    #[test]
    fn ganz_links_bleibt_nur_bass() {
        // Am Anschlag steht die Grenzfrequenz bei 40 Hz. Selbst 60 Hz liegt
        // dann schon darüber und wird gedämpft — das ist gewollt, „ganz zu"
        // heißt eben fast zu. Entscheidend ist der Abstand zu den Höhen.
        let bass = verhaeltnis(40.0, -1.0);
        let hoehen = verhaeltnis(8_000.0, -1.0);

        assert!(bass > 0.5, "Bass wird bei Tiefpass zu {bass:.3} gedämpft");
        assert!(
            hoehen < 0.02,
            "Höhen überleben den Tiefpass mit {hoehen:.3}"
        );
        assert!(
            bass / hoehen.max(1e-9) > 50.0,
            "Tiefpass trennt kaum: Bass {bass:.3} gegen Höhen {hoehen:.4}"
        );
    }

    #[test]
    fn ganz_rechts_bleibt_nur_hoehe() {
        let bass = verhaeltnis(60.0, 1.0);
        let hoehen = verhaeltnis(14_000.0, 1.0);

        assert!(hoehen > 0.5, "Höhen bei Hochpass nur {hoehen:.3}");
        assert!(bass < 0.02, "Bass überlebt den Hochpass mit {bass:.3}");
    }

    #[test]
    fn position_wird_geklemmt() {
        let mut f = DjFilter::new(48_000.0);
        f.set_position(-9.0);
        assert_eq!(f.position(), -1.0);
        f.set_position(9.0);
        assert_eq!(f.position(), 1.0);
    }
}
