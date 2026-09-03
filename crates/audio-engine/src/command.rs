//! Kommandos von der Oberfläche zum Audio-Thread.
//!
//! Der Mixer gehört dem Audio-Callback allein. Die Oberfläche fasst ihn nicht
//! an, sondern schickt Kommandos durch eine lock-freie Schlange, die der
//! Callback zu Beginn jedes Blocks leert.
//!
//! Ein `Mutex<Engine>` wäre bequemer und wäre falsch: Der Callback müsste dann
//! auf die Oberfläche warten, und Warten im Audio-Pfad heißt Aussetzer. Genau
//! deshalb steht diese Schlange in `docs/PLAN.md` als eine der drei
//! Entscheidungen, die die Architektur prägen.
//!
//! Transportbefehle (Play, Tempo, Springen) laufen **nicht** hierüber — die
//! liegen als Atomics im `DeckState` und sind von beiden Seiten direkt
//! erreichbar. Hier steht nur, was den Mixer selbst betrifft.

use rtrb::{Consumer, Producer, RingBuffer};

use crate::crossfader::Assign;
use crate::mixer::Engine;
use crate::source::Source;

/// Eine Änderung am Mixer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Trim(usize, f32),
    Fader(usize, f32),
    Eq {
        channel: usize,
        low: f32,
        mid: f32,
        high: f32,
    },
    Filter(usize, f32),
    Cue(usize, bool),
    Fx(usize, crate::effects::Effekt),
    FxMix(usize, f32),
    FxAmount(usize, f32),
    /// Effektzeit in Sekunden.
    FxTime(usize, f32),
    Assign(usize, Assign),
    Crossfader(f32),
    CrossfaderCurve(f32),
    MasterGain(f32),
    CueGain(f32),
    CueMix(f32),
}

/// Die Seite der Oberfläche.
pub struct EngineHandle {
    tx: Producer<Command>,
    loads: Producer<LoadRequest>,
    retired: Consumer<Box<dyn Source>>,
    dropped: u64,
}

impl EngineHandle {
    /// Legt einen Track auf einen Kanal. Die alte Quelle kommt über
    /// [`Self::collect_retired`] zurück und stirbt dort, nicht im Audio-Thread.
    pub fn load(&mut self, channel: usize, source: Box<dyn Source>) -> bool {
        self.loads.push(LoadRequest { channel, source }).is_ok()
    }

    /// Holt abgelöste Quellen ab und gibt sie frei.
    ///
    /// Muss regelmäßig aufgerufen werden — sonst verstopft der Rückweg, und
    /// der Audio-Thread nimmt keine neuen Tracks mehr an. Einmal pro Bild der
    /// Oberfläche reicht.
    pub fn collect_retired(&mut self) -> usize {
        let mut n = 0;
        while let Ok(alt) = self.retired.pop() {
            drop(alt);
            n += 1;
        }
        n
    }
    /// Schickt ein Kommando. Ist die Schlange voll, geht es verloren und wird
    /// gezählt — ein Reglerwert, der nicht ankommt, wird beim nächsten Zug
    /// ohnehin überschrieben. Blockieren wäre hier schlimmer als verlieren.
    pub fn send(&mut self, command: Command) {
        if self.tx.push(command).is_err() {
            self.dropped += 1;
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Ein Track, der auf einen Kanal soll.
pub struct LoadRequest {
    pub channel: usize,
    pub source: Box<dyn Source>,
}

/// Die Seite des Audio-Threads. Besitzt den Mixer.
pub struct EngineRunner {
    engine: Engine,
    rx: Consumer<Command>,
    loads: Consumer<LoadRequest>,
    retire: Producer<Box<dyn Source>>,
    /// Eine abgelöste Quelle, die noch nicht hinausgereicht werden konnte.
    pending: Option<Box<dyn Source>>,
}

impl EngineRunner {
    /// Kommandos übernehmen und einen Block rendern.
    pub fn render(&mut self, out: &mut [f32], out_channels: usize) {
        self.drain();
        self.engine.render(out, out_channels);
    }

    fn drain(&mut self) {
        while let Ok(command) = self.rx.pop() {
            self.apply(command);
        }
        self.drain_loads();
    }

    /// Nimmt neue Quellen entgegen und reicht die abgelösten hinaus.
    ///
    /// Der Kern: Ein abgelöster Track hält hundert Megabyte. Fällt er hier,
    /// gibt der letzte `Arc` sie frei — ein Syscall mitten im Callback und
    /// damit genau der Aussetzer, den die Architektur vermeidet.
    ///
    /// Deshalb wird nichts übernommen, solange der Rückweg verstopft ist. Die
    /// Anfrage bleibt dann in der Schlange und kommt einen Block später dran;
    /// das ist ein paar Millisekunden später und stört niemanden.
    fn drain_loads(&mut self) {
        if let Some(alt) = self.pending.take() {
            if let Err(rtrb::PushError::Full(zurueck)) = self.retire.push(alt) {
                self.pending = Some(zurueck);
                return;
            }
        }

        while !self.retire.is_full() {
            let Ok(request) = self.loads.pop() else {
                return;
            };
            if request.channel >= self.engine.channel_count() {
                // Auch diese Quelle darf hier nicht fallen. Der Rückweg ist an
                // dieser Stelle frei — die Schleifenbedingung hat es gerade
                // geprüft —, also geht sie sofort zurück statt zu warten.
                if let Err(rtrb::PushError::Full(zurueck)) = self.retire.push(request.source) {
                    self.pending = Some(zurueck);
                    return;
                }
                continue;
            }

            let alt = self
                .engine
                .channel(request.channel)
                .set_source(request.source);
            if let Err(rtrb::PushError::Full(zurueck)) = self.retire.push(alt) {
                self.pending = Some(zurueck);
                return;
            }
        }
    }

    fn apply(&mut self, command: Command) {
        let channels = self.engine.channel_count();
        let gueltig = |index: usize| index < channels;

        match command {
            Command::Trim(ch, value) if gueltig(ch) => self.engine.channel(ch).set_trim(value),
            Command::Fader(ch, value) if gueltig(ch) => self.engine.channel(ch).set_fader(value),
            Command::Eq {
                channel,
                low,
                mid,
                high,
            } if gueltig(channel) => self.engine.channel(channel).set_eq(low, mid, high),
            Command::Filter(ch, value) if gueltig(ch) => self.engine.channel(ch).set_filter(value),
            Command::Cue(ch, on) if gueltig(ch) => self.engine.channel(ch).set_cue(on),
            Command::Fx(ch, effekt) if gueltig(ch) => {
                self.engine.channel(ch).fx().set_effekt(effekt)
            }
            Command::FxMix(ch, value) if gueltig(ch) => self.engine.channel(ch).fx().set_mix(value),
            Command::FxAmount(ch, value) if gueltig(ch) => {
                self.engine.channel(ch).fx().set_amount(value)
            }
            Command::FxTime(ch, value) if gueltig(ch) => {
                self.engine.channel(ch).fx().set_zeit(value)
            }
            Command::Assign(ch, assign) if gueltig(ch) => {
                self.engine.channel(ch).set_assign(assign)
            }
            Command::Crossfader(value) => self.engine.crossfader().set_position(value),
            Command::CrossfaderCurve(value) => self.engine.crossfader().set_curve(value),
            Command::MasterGain(value) => self.engine.set_master_gain(value),
            Command::CueGain(value) => self.engine.set_cue_gain(value),
            Command::CueMix(value) => self.engine.set_cue_mix(value),
            // Kommandos für nicht vorhandene Kanäle werden verworfen statt zu
            // paniken — die Oberfläche könnte einem Umbau hinterherhinken.
            _ => {}
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Ein Runner ohne Kanäle.
    ///
    /// Notnagel für den Fall, dass der echte Mixer in einem gescheiterten
    /// Stream steckengeblieben ist. Er rendert Stille, damit der Aufrufer
    /// weiterarbeiten kann statt zu paniken.
    pub fn leer() -> EngineRunner {
        let (_, runner) = channel(Engine::new(48_000.0), 8);
        runner
    }
}

/// Verbindet Oberfläche und Audio-Thread.
pub fn channel(engine: Engine, capacity: usize) -> (EngineHandle, EngineRunner) {
    let (tx, rx) = RingBuffer::new(capacity.max(8));
    // Der Rückweg ist etwas weiter als der Hinweg, damit er nie der Engpass ist.
    let (load_tx, load_rx) = RingBuffer::new(8);
    let (retire_tx, retire_rx) = RingBuffer::new(12);

    (
        EngineHandle {
            tx,
            loads: load_tx,
            retired: retire_rx,
            dropped: 0,
        },
        EngineRunner {
            engine,
            rx,
            loads: load_rx,
            retire: retire_tx,
            pending: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{LoopSource, SilentSource};
    use crate::testing::{rms_channel, sine_stereo};

    const RATE: f32 = 48_000.0;

    fn aufbau() -> (EngineHandle, EngineRunner) {
        let mut engine = Engine::new(RATE);
        let samples = sine_stereo(220.0, RATE as u32, 1.0)
            .into_iter()
            .map(|v| v * 0.4)
            .collect();
        engine.add_channel("A", Box::new(LoopSource::new(samples)));
        engine.add_channel("leer", Box::new(SilentSource));
        channel(engine, 64)
    }

    fn pegel(runner: &mut EngineRunner) -> f32 {
        let mut out = vec![0.0; 4_800 * 4];
        runner.render(&mut out, 4);
        rms_channel(&out, 0, 4)
    }

    #[test]
    fn kommandos_kommen_an() {
        let (mut handle, mut runner) = aufbau();
        assert!(pegel(&mut runner) < 0.001, "Fader startet nicht unten");

        handle.send(Command::Fader(0, 1.0));
        assert!(pegel(&mut runner) > 0.1, "Fader-Kommando wirkt nicht");
    }

    #[test]
    fn kommandos_fuer_fehlende_kanaele_werden_verworfen() {
        let (mut handle, mut runner) = aufbau();
        handle.send(Command::Fader(99, 1.0));
        handle.send(Command::Eq {
            channel: 99,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
        });

        // Darf nicht paniken und darf nichts anderes verstellen.
        pegel(&mut runner);
    }

    #[test]
    fn eine_volle_schlange_verliert_statt_zu_blockieren() {
        let (mut handle, mut runner) = aufbau();
        for i in 0..10_000 {
            handle.send(Command::Crossfader(i as f32 * 1e-4));
        }

        assert!(handle.dropped() > 0, "die Schlange ist nicht übergelaufen");
        pegel(&mut runner);
    }

    #[test]
    fn der_letzte_wert_gewinnt() {
        let (mut handle, mut runner) = aufbau();
        handle.send(Command::Fader(0, 1.0));
        handle.send(Command::Fader(0, 0.0));

        assert!(
            pegel(&mut runner) < 0.001,
            "die Reihenfolge der Kommandos stimmt nicht"
        );
    }

    #[test]
    fn ein_geladener_track_ist_zu_hoeren() {
        let (mut handle, mut runner) = aufbau();
        handle.send(Command::Fader(1, 1.0));
        assert!(pegel(&mut runner) < 0.001, "der leere Kanal klingt");

        let samples = sine_stereo(220.0, RATE as u32, 1.0)
            .into_iter()
            .map(|v| v * 0.4)
            .collect();
        assert!(handle.load(1, Box::new(LoopSource::new(samples))));

        assert!(pegel(&mut runner) > 0.1, "die neue Quelle kam nicht an");
    }

    #[test]
    fn die_abgeloeste_quelle_kommt_zurueck_statt_im_callback_zu_sterben() {
        let (mut handle, mut runner) = aufbau();

        handle.load(0, Box::new(SilentSource));
        pegel(&mut runner);

        assert_eq!(
            handle.collect_retired(),
            1,
            "die alte Quelle wurde nicht herausgereicht"
        );
    }

    #[test]
    fn ohne_abholen_verstopft_der_rueckweg_statt_zu_verlieren() {
        // Wird nie abgeholt, muss der Audio-Thread irgendwann aufhören,
        // Tracks anzunehmen — und darf trotzdem nichts freigeben.
        let (mut handle, mut runner) = aufbau();

        for _ in 0..40 {
            handle.load(0, Box::new(SilentSource));
            pegel(&mut runner);
        }

        let zurueck = handle.collect_retired();
        assert!(zurueck > 0, "gar nichts kam zurück");
        assert!(
            zurueck <= 12,
            "mehr zurück als der Rückweg fasst: {zurueck}"
        );
    }

    #[test]
    fn laden_auf_einen_fehlenden_kanal_verliert_nichts() {
        let (mut handle, mut runner) = aufbau();
        handle.load(99, Box::new(SilentSource));
        pegel(&mut runner);

        assert_eq!(
            handle.collect_retired(),
            1,
            "die Quelle für den falschen Kanal ist verschwunden"
        );
    }

    #[test]
    fn eq_kommando_traegt_alle_drei_baender() {
        let (mut handle, mut runner) = aufbau();
        handle.send(Command::Fader(0, 1.0));
        let voll = pegel(&mut runner);

        handle.send(Command::Eq {
            channel: 0,
            low: 0.0,
            mid: 0.0,
            high: 0.0,
        });
        let zu = pegel(&mut runner);

        assert!(voll > 0.1);
        assert!(zu < voll * 0.1, "EQ-Kill kam nicht durch: {zu:.4}");
    }
}
