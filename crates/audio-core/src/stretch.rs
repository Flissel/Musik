//! WSOLA-Zeitstreckung — Tempo ändern, Tonhöhe behalten (Keylock).
//!
//! Das Verfahren schneidet überlappende Frames aus dem Quellmaterial und
//! überblendet sie mit einem Hann-Fenster. Damit an der Nahtstelle keine
//! Phasensprünge hörbar werden, wird die Schnittposition lokal um bis zu
//! `SEARCH` Samples verschoben — dorthin, wo die Kreuzkorrelation mit dem
//! bereits ausgegebenen Übergang am größten ist.
//!
//! Wichtig: Die *ideale* Leseposition läuft ungestört weiter, die Suche ist nur
//! eine lokale Korrektur. Würde man den gefundenen Versatz aufaddieren, driftete
//! die Wiedergabe über die Zeit aus dem Takt.
//!
//! Qualität: brauchbar für einen Prototyp und für die üblichen ±8 % im DJ-Betrieb.
//! Für extreme Faktoren oder Produktionsqualität siehe `docs/BAUSTEINE.md` —
//! Rubber Band oder signalsmith-stretch spielen in einer anderen Liga.

use crate::track::CHANNELS;

const FRAME: usize = 2048;
const HOP: usize = FRAME / 2;
const OVERLAP: usize = FRAME - HOP;
const SEARCH: usize = 256;
const CORR_LEN: usize = 256;

pub struct Wsola {
    window: Vec<f32>,
    /// Overlap-Add-Akkumulator, `FRAME` Frames à `CHANNELS`.
    accum: Vec<f32>,
    /// Fertig produzierter, noch nicht abgeholter Hop.
    fifo: Vec<f32>,
    fifo_read: usize,
}

impl Default for Wsola {
    fn default() -> Self {
        Self::new()
    }
}

impl Wsola {
    pub fn new() -> Self {
        // Periodisches Hann-Fenster: bei 50 % Überlappung summieren sich zwei
        // benachbarte Fenster exakt zu 1.0, es braucht also keine Normierung.
        let window = (0..FRAME)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FRAME as f32).cos())
            .collect();

        Wsola {
            window,
            accum: vec![0.0; FRAME * CHANNELS],
            fifo: vec![0.0; HOP * CHANNELS],
            fifo_read: HOP * CHANNELS,
        }
    }

    /// Verwirft den internen Zustand — nach einem Sprung oder Moduswechsel.
    pub fn reset(&mut self) {
        self.accum.fill(0.0);
        self.fifo_read = self.fifo.len();
    }

    /// Füllt `out` (interleaved Stereo) aus `src`.
    ///
    /// `pos` ist die Leseposition in Quell-Frames und wird fortgeschrieben,
    /// `ratio` das Tempoverhältnis (1.0 = Original). Gibt `false` zurück, wenn
    /// die Quelle erschöpft ist; der Rest von `out` ist dann Stille.
    pub fn render(&mut self, src: &[f32], ratio: f64, pos: &mut f64, out: &mut [f32]) -> bool {
        self.render_gemischt(src, std::slice::from_ref(&src), &[1.0], ratio, pos, out)
    }

    /// Dasselbe aus mehreren Spuren, jede mit ihrem Pegel.
    ///
    /// **Gesucht wird auf der Summe, angewandt auf jede Spur.** Das ist der
    /// Kern: Die Zeitstreckung sucht sich in jedem Hop die Stelle, an der die
    /// Wellenform am besten anschließt. Liefe diese Suche je Spur getrennt,
    /// fände jede eine andere — vier Spuren desselben Stücks liefen um
    /// Millisekunden auseinander, und was vorher zusammen klang, klänge
    /// verwaschen. Eine Entscheidung für alle, und `referenz` ist die Summe,
    /// die ohnehin schon dasteht.
    ///
    /// Gemischt wird beim Lesen und nicht danach. Das ist dasselbe Ergebnis —
    /// dieselbe Stelle, dasselbe Fenster —, kostet aber einen statt vier
    /// Akkumulatoren, und im Audio-Callback zählt das.
    pub fn render_gemischt(
        &mut self,
        referenz: &[f32],
        spuren: &[&[f32]],
        pegel: &[f32],
        ratio: f64,
        pos: &mut f64,
        out: &mut [f32],
    ) -> bool {
        let mut written = 0;

        while written < out.len() {
            if self.fifo_read < self.fifo.len() {
                let n = (out.len() - written).min(self.fifo.len() - self.fifo_read);
                out[written..written + n]
                    .copy_from_slice(&self.fifo[self.fifo_read..self.fifo_read + n]);
                written += n;
                self.fifo_read += n;
                continue;
            }

            if !self.produce_hop(referenz, spuren, pegel, ratio, pos) {
                out[written..].fill(0.0);
                return false;
            }
        }

        true
    }

    /// Erzeugt genau einen Hop Ausgangsmaterial in den FIFO.
    fn produce_hop(
        &mut self,
        referenz: &[f32],
        spuren: &[&[f32]],
        pegel: &[f32],
        ratio: f64,
        pos: &mut f64,
    ) -> bool {
        let src_frames = spuren
            .iter()
            .map(|s| s.len() / CHANNELS)
            .chain(std::iter::once(referenz.len() / CHANNELS))
            .min()
            .unwrap_or(0);
        if src_frames < FRAME {
            return false;
        }

        let ideal = pos.round();
        if ideal < 0.0 {
            return false;
        }
        let max_start = src_frames - FRAME;
        if ideal as usize > max_start {
            return false;
        }

        let start = self.best_offset(referenz, ideal as usize, max_start);

        for i in 0..FRAME {
            let w = self.window[i];
            let s = (start + i) * CHANNELS;
            let a = i * CHANNELS;
            for c in 0..CHANNELS {
                let mut summe = 0.0;
                for (spur, p) in spuren.iter().zip(pegel) {
                    summe += spur[s + c] * p;
                }
                self.accum[a + c] += summe * w;
            }
        }

        self.fifo.copy_from_slice(&self.accum[..HOP * CHANNELS]);
        self.fifo_read = 0;

        self.accum.copy_within(HOP * CHANNELS.., 0);
        self.accum[OVERLAP * CHANNELS..].fill(0.0);

        // Ideale Position fortschreiben, nicht die gefundene — sonst driftet es.
        *pos += HOP as f64 * ratio;
        true
    }

    /// Sucht im Fenster ±`SEARCH` um `ideal` die Position, deren Wellenform am
    /// besten an den bereits ausgegebenen Übergang anschließt.
    fn best_offset(&self, src: &[f32], ideal: usize, max_start: usize) -> usize {
        let lo = ideal.saturating_sub(SEARCH);
        let hi = (ideal + SEARCH).min(max_start);
        if lo >= hi {
            return ideal.min(max_start);
        }

        let mut best = ideal.min(max_start);
        let mut best_score = f32::NEG_INFINITY;

        for cand in lo..=hi {
            let mut dot = 0.0f32;
            let mut energy = 0.0f32;

            for i in 0..CORR_LEN {
                let a = i * CHANNELS;
                let reference = self.accum[a] + self.accum[a + 1];
                let s = (cand + i) * CHANNELS;
                let sample = src[s] + src[s + 1];
                dot += reference * sample;
                energy += sample * sample;
            }

            // Normieren, damit nicht einfach die lauteste Stelle gewinnt.
            let score = dot / (energy.sqrt() + 1e-9);
            if score > best_score {
                best_score = score;
                best = cand;
            }
        }

        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{dominant_freq, sine};

    const RATE: u32 = 44_100;

    /// Der Kern von Keylock: Tempo ändern, Tonhöhe halten.
    #[test]
    fn keylock_haelt_die_tonhoehe() {
        for ratio in [0.92_f64, 1.06, 1.20] {
            let src = sine(440.0, RATE, 10.0);
            let mut pos = 0.0;
            let mut wsola = Wsola::new();
            let mut out = vec![0.0; 4 * RATE as usize * CHANNELS];

            assert!(
                wsola.render(&src, ratio, &mut pos, &mut out),
                "Quelle war zu früh erschöpft bei ratio={ratio}"
            );

            let freq = dominant_freq(&out, RATE);
            assert!(
                (freq - 440.0).abs() < 13.0,
                "ratio={ratio}: erwartet ~440 Hz, gemessen {freq:.1} Hz"
            );
        }
    }

    /// Bei ratio=1.06 müssen 4 s Ausgabe rund 4,24 s Quelle verbrauchen.
    #[test]
    fn zeitachse_stimmt() {
        let src = sine(440.0, RATE, 20.0);
        let mut pos = 0.0;
        let mut wsola = Wsola::new();
        let out_frames = 4 * RATE as usize;
        let mut out = vec![0.0; out_frames * CHANNELS];

        assert!(wsola.render(&src, 1.06, &mut pos, &mut out));

        let erwartet = out_frames as f64 * 1.06;
        // Toleranz: ein Hop, der wegen des FIFO noch nicht ausgegeben wurde.
        assert!(
            (pos - erwartet).abs() < (FRAME as f64),
            "Position {pos:.0} weicht von erwarteten {erwartet:.0} ab"
        );
    }

    /// Läuft die Quelle aus, meldet render() das und liefert Stille statt Müll.
    #[test]
    fn quellenende_wird_gemeldet() {
        let src = sine(440.0, RATE, 0.5);
        let mut pos = 0.0;
        let mut wsola = Wsola::new();
        let mut out = vec![0.123; 2 * RATE as usize * CHANNELS];

        assert!(!wsola.render(&src, 1.0, &mut pos, &mut out));
        assert_eq!(
            out.last().copied(),
            Some(0.0),
            "nach Quellenende muss Stille stehen"
        );
    }

    /// Ein zu kurzer Puffer darf nicht paniken.
    #[test]
    fn zu_kurze_quelle_paniert_nicht() {
        let src = sine(440.0, RATE, 0.001);
        let mut pos = 0.0;
        let mut wsola = Wsola::new();
        let mut out = vec![0.0; 512 * CHANNELS];

        assert!(!wsola.render(&src, 1.0, &mut pos, &mut out));
    }
}
