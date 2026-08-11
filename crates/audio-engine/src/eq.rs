//! Dreiband-EQ mit Kill.
//!
//! Kein Shelving-EQ, sondern ein echtes Crossover: Das Signal wird bei 200 Hz
//! und 2 kHz aufgeteilt, die drei Bänder werden getrennt verstärkt und wieder
//! summiert.
//!
//! Der Unterschied ist nicht akademisch. Ein Shelving-Filter kann ein Band
//! absenken, aber nie ganz entfernen — und ein DJ-EQ, dessen Bass-Regler den
//! Bass nicht *weg* bekommt, ist für Übergänge unbrauchbar. Mit Aufteilung ist
//! Verstärkung null gleichbedeutend mit Kill.
//!
//! ## Zwei Sackgassen, die hier schon durchlaufen sind
//!
//! **Tief- und Hochpass eines Zustandsvariablen-Filters** summieren sich nicht
//! zum Eingang — es fehlt der Bandpassanteil `k·bp`. In Neutralstellung war der
//! EQ dadurch kein Durchgang, sondern eine Kerbe um die Trennfrequenz; ein
//! 220-Hz-Ton verlor über 80 % seines Pegels.
//!
//! **Das Restband subtraktiv bilden** (`rest = Eingang − tief`) ist im
//! Zeitbereich zwar exakt, aber `tief` ist phasenverschoben. Bei 60 Hz liegt
//! die Verschiebung eines Filters vierter Ordnung schon bei etwa 50°, und
//! `|Eingang − tief|` wurde dadurch 0,84 statt nahe null — der Bass-Kill ließ
//! den Bass stehen. Auf der Trennfrequenz kam sogar das Anderthalbfache heraus.
//!
//! ## Was stattdessen gilt
//!
//! Linkwitz-Riley vierter Ordnung: je Trennstelle ein Tief- und ein Hochpass
//! aus zwei kaskadierten Butterworth-Stufen. Genau dafür ist diese Bauform
//! gemacht — beide Zweige liegen auf der Trennfrequenz *in Phase* bei −6 dB und
//! ergeben zusammen wieder eins. Die Summe aller drei Bänder ist ein Allpass:
//! Betrag glatt, Phase gedreht. Für einen EQ ist das genau richtig.

use crate::svf::Svf;

const LOW_MID_HZ: f32 = 200.0;
const MID_HIGH_HZ: f32 = 2_000.0;
const Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Maximale Anhebung je Band (etwa +6 dB).
pub const MAX_GAIN: f32 = 2.0;

/// Linkwitz-Riley-Trennstelle vierter Ordnung, mono.
#[derive(Debug, Clone, Copy)]
struct Crossover {
    lp1: Svf,
    lp2: Svf,
    hp1: Svf,
    hp2: Svf,
}

impl Crossover {
    fn new() -> Self {
        Crossover {
            lp1: Svf::new(),
            lp2: Svf::new(),
            hp1: Svf::new(),
            hp2: Svf::new(),
        }
    }

    fn set(&mut self, cutoff: f32, sample_rate: f32) {
        for svf in [&mut self.lp1, &mut self.lp2, &mut self.hp1, &mut self.hp2] {
            svf.set(cutoff, Q, sample_rate);
        }
    }

    fn reset(&mut self) {
        for svf in [&mut self.lp1, &mut self.lp2, &mut self.hp1, &mut self.hp2] {
            svf.reset();
        }
    }

    /// Liefert (unterhalb, oberhalb).
    #[inline]
    fn process(&mut self, input: f32) -> (f32, f32) {
        let low = self.lp2.process(self.lp1.process(input).low).low;
        let high = self.hp2.process(self.hp1.process(input).high).high;
        (low, high)
    }
}

#[derive(Debug, Clone)]
pub struct ThreeBandEq {
    /// Pro Kanal eine eigene Trennstelle — Filter haben Zustand.
    lower: [Crossover; 2],
    upper: [Crossover; 2],
    low: f32,
    mid: f32,
    high: f32,
}

impl ThreeBandEq {
    pub fn new(sample_rate: f32) -> Self {
        let mut eq = ThreeBandEq {
            lower: [Crossover::new(); 2],
            upper: [Crossover::new(); 2],
            low: 1.0,
            mid: 1.0,
            high: 1.0,
        };
        eq.set_sample_rate(sample_rate);
        eq
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        for split in self.lower.iter_mut() {
            split.set(LOW_MID_HZ, sample_rate);
        }
        for split in self.upper.iter_mut() {
            split.set(MID_HIGH_HZ, sample_rate);
        }
    }

    /// Verstärkungen der drei Bänder. 0.0 = Kill, 1.0 = neutral.
    pub fn set_gains(&mut self, low: f32, mid: f32, high: f32) {
        self.low = low.clamp(0.0, MAX_GAIN);
        self.mid = mid.clamp(0.0, MAX_GAIN);
        self.high = high.clamp(0.0, MAX_GAIN);
    }

    pub fn gains(&self) -> (f32, f32, f32) {
        (self.low, self.mid, self.high)
    }

    pub fn is_neutral(&self) -> bool {
        (self.low - 1.0).abs() < f32::EPSILON
            && (self.mid - 1.0).abs() < f32::EPSILON
            && (self.high - 1.0).abs() < f32::EPSILON
    }

    pub fn reset(&mut self) {
        for split in self.lower.iter_mut().chain(self.upper.iter_mut()) {
            split.reset();
        }
    }

    /// Verarbeitet interleaved Stereo an Ort und Stelle.
    pub fn process(&mut self, buffer: &mut [f32]) {
        for frame in buffer.chunks_exact_mut(2) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let (low, rest) = self.lower[ch].process(*sample);
                let (mid, high) = self.upper[ch].process(rest);

                *sample = low * self.low + mid * self.mid + high * self.high;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{rms, sine_stereo};

    const RATE: u32 = 48_000;

    /// Verhältnis von Ausgang zu Eingang bei einer Frequenz, mit gegebenen Reglern.
    fn verhaeltnis(freq: f32, low: f32, mid: f32, high: f32) -> f32 {
        let signal = sine_stereo(freq, RATE, 0.5);
        let mut eq = ThreeBandEq::new(RATE as f32);
        eq.set_gains(low, mid, high);

        let mut out = signal.clone();
        eq.process(&mut out);

        let half = out.len() / 2;
        rms(&out[half..]) / rms(&signal[half..])
    }

    #[test]
    fn neutral_ist_ein_echter_durchgang() {
        // Die Eigenschaft, an der beide früheren Entwürfe gescheitert sind.
        for freq in [60.0, 200.0, 630.0, 2_000.0, 8_000.0] {
            let v = verhaeltnis(freq, 1.0, 1.0, 1.0);
            assert!(
                (v - 1.0).abs() < 0.02,
                "{freq} Hz wird neutral auf {v:.4} verändert"
            );
        }
    }

    #[test]
    fn bass_kill_entfernt_den_bass() {
        let bass = verhaeltnis(60.0, 0.0, 1.0, 1.0);
        assert!(bass < 0.05, "60 Hz überlebt den Kill mit {bass:.3}");
    }

    #[test]
    fn bass_kill_laesst_die_hoehen_stehen() {
        // Das ist der eigentliche Zweck: Bass raus, Rest bleibt spielbar.
        let hoehen = verhaeltnis(8_000.0, 0.0, 1.0, 1.0);
        assert!(hoehen > 0.95, "Bass-Kill dämpft auch 8 kHz auf {hoehen:.3}");
    }

    #[test]
    fn jedes_band_trifft_seinen_bereich() {
        assert!(
            verhaeltnis(60.0, 0.0, 1.0, 1.0) < 0.05,
            "Low-Kill wirkt nicht"
        );
        assert!(
            verhaeltnis(630.0, 1.0, 0.0, 1.0) < 0.05,
            "Mid-Kill wirkt nicht"
        );
        assert!(
            verhaeltnis(10_000.0, 1.0, 1.0, 0.0) < 0.05,
            "High-Kill wirkt nicht"
        );
    }

    #[test]
    fn auf_der_trennfrequenz_teilen_sich_zwei_baender_das_signal() {
        // Bei Linkwitz-Riley liegt dort jeder Zweig bei −6 dB. Kill des einen
        // lässt also etwa die Hälfte stehen — kein Fehler, sondern die
        // Bauform. Bei jedem analogen DJ-EQ ist es genauso.
        let auf_der_grenze = verhaeltnis(200.0, 0.0, 1.0, 1.0);
        assert!(
            (0.35..0.65).contains(&auf_der_grenze),
            "auf der Trennfrequenz unerwartet: {auf_der_grenze:.3}"
        );
    }

    #[test]
    fn alles_zu_ergibt_stille() {
        let v = verhaeltnis(1_000.0, 0.0, 0.0, 0.0);
        assert!(v < 0.02, "trotz aller Kills bleibt {v:.3}");
    }

    #[test]
    fn anhebung_ist_begrenzt() {
        let mut eq = ThreeBandEq::new(RATE as f32);
        eq.set_gains(99.0, -5.0, 1.5);
        let (low, mid, high) = eq.gains();

        assert_eq!(low, MAX_GAIN);
        assert_eq!(mid, 0.0);
        assert_eq!(high, 1.5);
        assert!(!eq.is_neutral());
    }
}
