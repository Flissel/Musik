//! Zustandsvariablen-Filter (TPT, nach Andy Simper).
//!
//! Baustein für EQ und Filterknopf. Liefert Tief-, Band- und Hochpass aus
//! *einem* Durchlauf — genau das, was ein Crossover braucht, ohne das Signal
//! mehrfach durch getrennte Filter zu schicken.
//!
//! Die topologiebewahrende Form bleibt auch bei Parameteränderungen während
//! des Betriebs stabil. Das ist keine Feinheit: An einem DJ-Filterknopf wird
//! gedreht, während Audio läuft, und ein klassisches Biquad kann dabei knacken.

use std::f32::consts::PI;

#[derive(Debug, Clone, Copy)]
pub struct Outputs {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Default for Svf {
    fn default() -> Self {
        Self::new()
    }
}

impl Svf {
    pub fn new() -> Self {
        let mut svf = Svf {
            ic1: 0.0,
            ic2: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
            k: 0.0,
        };
        svf.set(1_000.0, std::f32::consts::FRAC_1_SQRT_2, 48_000.0);
        svf
    }

    /// Setzt Grenzfrequenz und Güte. `q` = 0.707 ergibt Butterworth.
    pub fn set(&mut self, cutoff_hz: f32, q: f32, sample_rate: f32) {
        let nyquist = sample_rate * 0.5;
        let cutoff = cutoff_hz.clamp(10.0, nyquist * 0.99);
        let g = (PI * cutoff / sample_rate).tan();
        let k = 1.0 / q.max(0.01);

        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
        self.k = k;
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, input: f32) -> Outputs {
        let v3 = input - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;

        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        Outputs {
            low: v2,
            band: v1,
            high: input - self.k * v1 - v2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{rms, sine};

    const RATE: f32 = 48_000.0;

    fn durchlass(cutoff: f32, freq: f32, band: fn(Outputs) -> f32) -> f32 {
        let signal = sine(freq, RATE as u32, 0.5);
        let mut svf = Svf::new();
        svf.set(cutoff, std::f32::consts::FRAC_1_SQRT_2, RATE);

        // Erste Hälfte verwerfen, damit der Einschwingvorgang draußen bleibt.
        let out: Vec<f32> = signal.iter().map(|x| band(svf.process(*x))).collect();
        rms(&out[out.len() / 2..]) / rms(&signal[signal.len() / 2..])
    }

    #[test]
    fn tiefpass_laesst_tiefes_durch_und_sperrt_hohes() {
        let durch = durchlass(1_000.0, 100.0, |o| o.low);
        let gesperrt = durchlass(1_000.0, 10_000.0, |o| o.low);

        assert!(durch > 0.9, "100 Hz nur bei {durch:.3} durchgelassen");
        assert!(gesperrt < 0.05, "10 kHz noch bei {gesperrt:.3}");
    }

    #[test]
    fn hochpass_ist_das_spiegelbild() {
        let gesperrt = durchlass(1_000.0, 100.0, |o| o.high);
        let durch = durchlass(1_000.0, 10_000.0, |o| o.high);

        assert!(durch > 0.9, "10 kHz nur bei {durch:.3}");
        assert!(gesperrt < 0.05, "100 Hz noch bei {gesperrt:.3}");
    }

    #[test]
    fn an_der_grenzfrequenz_bleibt_die_haelfte_der_leistung() {
        let durch = durchlass(1_000.0, 1_000.0, |o| o.low);
        // −3 dB entspricht Faktor 0.707.
        assert!(
            (durch - 0.707).abs() < 0.05,
            "bei fc gemessen: {durch:.3}, erwartet ~0.707"
        );
    }

    #[test]
    fn bleibt_bei_extremen_parametern_endlich() {
        let mut svf = Svf::new();
        for cutoff in [0.0, 1.0, 24_000.0, 1e9] {
            for q in [0.0, 0.1, 100.0] {
                svf.set(cutoff, q, RATE);
                svf.reset();
                for i in 0..1_000 {
                    let o = svf.process(if i % 2 == 0 { 1.0 } else { -1.0 });
                    assert!(
                        o.low.is_finite() && o.band.is_finite() && o.high.is_finite(),
                        "divergiert bei cutoff={cutoff}, q={q}"
                    );
                }
            }
        }
    }
}
