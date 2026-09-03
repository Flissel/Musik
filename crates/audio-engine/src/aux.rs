//! AUX-Eingang: externes Audio in den Mixer.
//!
//! Mikrofon, Drum-Machine, ein zweiter Rechner, das Handy eines Gastes — alles,
//! was nicht aus einem Deck kommt, läuft hier herein. Im Mixer ist AUX ein
//! Kanal wie jeder andere: mit Trim, EQ, Filter, Fader und Cue.
//!
//! ## Warum ein Ringpuffer
//!
//! Die Aufnahme läuft in ihrem eigenen Callback, die Wiedergabe in einem
//! anderen. Zwei Callbacks heißt zwei Uhren, und Uhren laufen nie exakt gleich.
//! Ein lock-freier Ringpuffer entkoppelt beide, sodass keiner auf den anderen
//! wartet — im Audio-Pfad ist Warten dasselbe wie ein Aussetzer.
//!
//! Bleibt die Aufnahme zurück, wird Stille ausgegeben und ein Unterlauf
//! gezählt. Das ist eine Diagnose, kein Zufall: Häufige Unterläufe bedeuten,
//! dass der Puffer zu klein ist oder die beiden Uhren zu weit auseinander
//! driften.

use rtrb::{Consumer, Producer, RingBuffer};

use crate::source::Source;

/// Schreibende Seite — gehört dem Aufnahme-Callback.
pub struct AuxWriter {
    producer: Producer<f32>,
    overruns: u64,
}

impl AuxWriter {
    /// Schiebt interleaved Stereo hinein. Gibt zurück, wie viele Werte Platz
    /// fanden; der Rest geht verloren und zählt als Überlauf.
    pub fn write(&mut self, samples: &[f32]) -> usize {
        let mut written = 0;
        for sample in samples {
            if self.producer.push(*sample).is_err() {
                self.overruns += samples.len() as u64 - written as u64;
                break;
            }
            written += 1;
        }
        written
    }

    /// Wie viele Werte verworfen wurden, weil der Puffer voll war.
    pub fn overruns(&self) -> u64 {
        self.overruns
    }
}

/// Lesende Seite — hängt als Quelle im Mixer.
pub struct AuxSource {
    consumer: Consumer<f32>,
    underruns: u64,
}

impl AuxSource {
    /// Wie viele Werte als Stille geliefert wurden, weil nichts anlag.
    pub fn underruns(&self) -> u64 {
        self.underruns
    }

    /// Wie viele Werte gerade bereitliegen.
    pub fn available(&self) -> usize {
        self.consumer.slots()
    }
}

impl Source for AuxSource {
    fn render(&mut self, out: &mut [f32]) {
        for (i, sample) in out.iter_mut().enumerate() {
            match self.consumer.pop() {
                Ok(value) => *sample = value,
                Err(_) => {
                    self.underruns += (out.len() - i) as u64;
                    out[i..].fill(0.0);
                    return;
                }
            }
        }
    }
}

/// Legt einen AUX-Kanal an. `capacity_frames` ist die Puffertiefe in
/// Stereo-Frames — größer heißt robuster gegen Uhrendrift, aber auch mehr
/// Latenz zwischen Eingang und Ausgang.
pub fn aux_channel(capacity_frames: usize) -> (AuxWriter, AuxSource) {
    let (producer, consumer) = RingBuffer::new(capacity_frames.max(1) * 2);
    (
        AuxWriter {
            producer,
            overruns: 0,
        },
        AuxSource {
            consumer,
            underruns: 0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geschriebenes_kommt_unveraendert_an() {
        let (mut writer, mut source) = aux_channel(1_024);

        let eingang: Vec<f32> = (0..64).map(|i| i as f32 * 0.01).collect();
        assert_eq!(writer.write(&eingang), eingang.len());

        let mut out = vec![0.0; 64];
        source.render(&mut out);

        assert_eq!(out, eingang);
        assert_eq!(source.underruns(), 0);
    }

    #[test]
    fn unterlauf_liefert_stille_und_wird_gezaehlt() {
        let (mut writer, mut source) = aux_channel(1_024);
        writer.write(&[0.5; 8]);

        let mut out = vec![9.0; 32];
        source.render(&mut out);

        assert_eq!(&out[..8], &[0.5; 8]);
        assert!(
            out[8..].iter().all(|v| *v == 0.0),
            "nach dem Unterlauf steht kein Nullpegel"
        );
        assert_eq!(source.underruns(), 24);
    }

    #[test]
    fn ohne_zuspieler_ist_der_kanal_still() {
        let (_writer, mut source) = aux_channel(64);
        let mut out = vec![1.0; 16];
        source.render(&mut out);

        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(source.underruns(), 16);
    }

    #[test]
    fn ueberlauf_wird_gezaehlt_statt_zu_blockieren() {
        let (mut writer, _source) = aux_channel(8);
        let geschrieben = writer.write(&[1.0; 64]);

        assert!(
            geschrieben < 64,
            "der Puffer hat mehr geschluckt als er fasst"
        );
        assert!(writer.overruns() > 0, "Überlauf wurde nicht bemerkt");
    }

    #[test]
    fn kapazitaet_null_paniert_nicht() {
        let (mut writer, mut source) = aux_channel(0);
        writer.write(&[1.0; 4]);
        let mut out = vec![0.0; 4];
        source.render(&mut out);
    }
}
