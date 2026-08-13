//! Der Mixer: summiert Kanäle auf Master- und Cue-Bus.
//!
//! Zwei getrennte Summen, zwei getrennte Ausgänge. Der Master geht auf die
//! Anlage (Ausgang 1/2), der Cue auf den Kopfhörer (Ausgang 3/4) — beides über
//! *dasselbe* Gerät, weil zwei Geräte zwei Uhren bedeuten und deren Drift den
//! Kopfhörer nach ein paar Minuten hörbar gegen den Master verschiebt. Siehe
//! `docs/PLAN.md`.

use crate::channel::Channel;
use crate::crossfader::{Assign, Crossfader};
use crate::limiter::Limiter;
use crate::source::Source;

pub struct Engine {
    sample_rate: f32,
    channels: Vec<Channel>,
    crossfader: Crossfader,
    limiter: Limiter,
    master_gain: f32,
    cue_gain: f32,
    cue_mix: f32,
    master: Vec<f32>,
    cue: Vec<f32>,
    /// Zwischenspeicher für das Signal hinter dem Fader.
    post: Vec<f32>,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        Engine {
            sample_rate,
            channels: Vec::new(),
            crossfader: Crossfader::new(),
            limiter: Limiter::new(sample_rate),
            master_gain: 1.0,
            cue_gain: 1.0,
            cue_mix: 0.0,
            master: Vec::new(),
            cue: Vec::new(),
            post: Vec::new(),
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Legt einen Kanal an und gibt seinen Index zurück.
    pub fn add_channel(&mut self, name: impl Into<String>, source: Box<dyn Source>) -> usize {
        self.channels
            .push(Channel::new(name, source, self.sample_rate));
        self.channels.len() - 1
    }

    pub fn channel(&mut self, index: usize) -> &mut Channel {
        &mut self.channels[index]
    }

    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn crossfader(&mut self) -> &mut Crossfader {
        &mut self.crossfader
    }

    pub fn limiter(&self) -> &Limiter {
        &self.limiter
    }

    pub fn set_master_gain(&mut self, gain: f32) {
        self.master_gain = gain.clamp(0.0, 2.0);
    }

    pub fn set_cue_gain(&mut self, gain: f32) {
        self.cue_gain = gain.clamp(0.0, 2.0);
    }

    /// Blendet den Master in den Kopfhörer: 0.0 = nur Cue, 1.0 = nur Master.
    pub fn set_cue_mix(&mut self, mix: f32) {
        self.cue_mix = mix.clamp(0.0, 1.0);
    }

    /// Wieviele Ausgangskanäle nötig sind, damit der Cue-Bus ankommt.
    pub const REQUIRED_OUTPUTS_FOR_CUE: usize = 4;

    /// Rendert einen Block in den Gerätepuffer.
    ///
    /// Belegung: 0/1 Master, 2/3 Cue. Hat das Gerät weniger als vier Kanäle,
    /// fällt der Cue-Bus weg — die Software läuft dann, aber Vorhören ist
    /// nicht möglich.
    pub fn render(&mut self, out: &mut [f32], out_channels: usize) {
        if out_channels == 0 {
            return;
        }
        let frames = out.len() / out_channels;
        let len = frames * 2;

        if self.master.len() < len {
            self.master.resize(len, 0.0);
            self.cue.resize(len, 0.0);
            self.post.resize(len, 0.0);
        }
        self.master[..len].fill(0.0);
        self.cue[..len].fill(0.0);

        for channel in self.channels.iter_mut() {
            let cross = self.crossfader.gain(channel.assign());
            let cued = channel.is_cued();
            // Ein Kanal mit klingendem Effekt ist nicht still, auch wenn sein
            // Fader unten ist — sonst risse jedes Zuziehen die Delayfahne ab.
            let hoerbar = cross > 0.0 && (channel.fader() > 0.0 || channel.klingt_nach());

            if !hoerbar && !cued {
                // Still und nicht im Kopfhörer — die Kette trotzdem laufen
                // lassen wäre Rechenzeit ohne Wirkung. Der Filterzustand
                // altert dabei, was beim Aufziehen einen Einschwinger kostet;
                // das ist der Preis und er ist kurz.
                continue;
            }

            {
                let pre = channel.render_pre_fader(frames);

                if cued {
                    for (dst, src) in self.cue[..len].iter_mut().zip(pre) {
                        *dst += *src;
                    }
                }
                self.post[..len].copy_from_slice(pre);
            }

            if !hoerbar {
                continue;
            }

            // Fader und Effekte, dann erst der Crossfader.
            channel.process_post_fader(&mut self.post[..len]);
            for (dst, src) in self.master[..len].iter_mut().zip(&self.post[..len]) {
                *dst += *src * cross;
            }
        }

        if (self.master_gain - 1.0).abs() > f32::EPSILON {
            for sample in self.master[..len].iter_mut() {
                *sample *= self.master_gain;
            }
        }
        self.limiter.process(&mut self.master[..len]);

        // Kopfhörer: Mischung aus Cue-Bus und Master.
        for i in 0..len {
            let mixed = self.cue[i] * (1.0 - self.cue_mix) + self.master[i] * self.cue_mix;
            self.cue[i] = (mixed * self.cue_gain).clamp(-1.0, 1.0);
        }

        for frame in 0..frames {
            let base = frame * out_channels;
            let slot = &mut out[base..base + out_channels];
            slot.fill(0.0);

            match out_channels {
                1 => slot[0] = 0.5 * (self.master[frame * 2] + self.master[frame * 2 + 1]),
                2 | 3 => {
                    slot[0] = self.master[frame * 2];
                    slot[1] = self.master[frame * 2 + 1];
                }
                _ => {
                    slot[0] = self.master[frame * 2];
                    slot[1] = self.master[frame * 2 + 1];
                    slot[2] = self.cue[frame * 2];
                    slot[3] = self.cue[frame * 2 + 1];
                }
            }
        }
    }
}

/// Bequemer Zugriff auf die Crossfader-Seiten beim Aufbau.
pub fn assign_deck_pair(engine: &mut Engine, left: usize, right: usize) {
    engine.channel(left).set_assign(Assign::A);
    engine.channel(right).set_assign(Assign::B);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{LoopSource, SilentSource};
    use crate::testing::{peak, rms, rms_channel, sine_stereo};

    const RATE: f32 = 48_000.0;
    const FRAMES: usize = 4_800;

    fn ton(freq: f32, amplitude: f32) -> Box<dyn Source> {
        let samples = sine_stereo(freq, RATE as u32, 1.0)
            .into_iter()
            .map(|v| v * amplitude)
            .collect();
        Box::new(LoopSource::new(samples))
    }

    fn rendern(engine: &mut Engine, out_channels: usize) -> Vec<f32> {
        let mut out = vec![0.0; FRAMES * out_channels];
        engine.render(&mut out, out_channels);
        out
    }

    fn master_rms(out: &[f32], channels: usize) -> f32 {
        rms_channel(out, 0, channels)
    }

    fn cue_rms(out: &[f32], channels: usize) -> f32 {
        rms_channel(out, 2, channels)
    }

    #[test]
    fn ohne_kanaele_bleibt_es_still() {
        let mut engine = Engine::new(RATE);
        let out = rendern(&mut engine, 4);
        assert_eq!(peak(&out), 0.0);
    }

    #[test]
    fn zwei_decks_summieren_sich() {
        let mut engine = Engine::new(RATE);
        let a = engine.add_channel("A", ton(220.0, 0.3));
        let b = engine.add_channel("B", ton(660.0, 0.3));

        engine.channel(a).set_fader(1.0);
        let nur_a = master_rms(&rendern(&mut engine, 4), 4);

        engine.channel(b).set_fader(1.0);
        let beide = master_rms(&rendern(&mut engine, 4), 4);

        assert!(
            beide > nur_a * 1.2,
            "zweites Deck kommt nicht an: {nur_a:.4} → {beide:.4}"
        );
    }

    #[test]
    fn der_crossfader_blendet_zwischen_den_seiten() {
        let mut engine = Engine::new(RATE);
        let a = engine.add_channel("A", ton(220.0, 0.4));
        let b = engine.add_channel("B", ton(220.0, 0.4));
        engine.channel(a).set_fader(1.0);
        engine.channel(b).set_fader(1.0);
        assign_deck_pair(&mut engine, a, b);

        engine.crossfader().set_position(-1.0);
        let links = master_rms(&rendern(&mut engine, 4), 4);

        engine.channel(a).set_fader(0.0);
        let links_ohne_a = master_rms(&rendern(&mut engine, 4), 4);

        assert!(links > 0.1, "auf A steht nichts an: {links:.4}");
        assert!(
            links_ohne_a < 0.01,
            "B ist bei Crossfader ganz links hörbar: {links_ohne_a:.4}"
        );
    }

    #[test]
    fn thru_kanaele_ueberleben_jede_crossfader_stellung() {
        // Genau dafür ist AUX da: ein Mikrofon darf nicht verschwinden,
        // nur weil der Crossfader bewegt wird.
        let mut engine = Engine::new(RATE);
        let aux = engine.add_channel("AUX", ton(440.0, 0.3));
        engine.channel(aux).set_fader(1.0);
        engine.channel(aux).set_assign(Assign::Thru);

        let mut pegel = Vec::new();
        for pos in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            engine.crossfader().set_position(pos);
            pegel.push(master_rms(&rendern(&mut engine, 4), 4));
        }

        let min = pegel.iter().cloned().fold(f32::MAX, f32::min);
        let max = pegel.iter().cloned().fold(0.0f32, f32::max);
        assert!(min > 0.1, "Thru-Kanal fällt weg: {pegel:?}");
        assert!(
            (max - min) / max < 0.02,
            "Thru-Kanal schwankt mit dem Crossfader: {pegel:?}"
        );
    }

    #[test]
    fn cue_hoert_einen_kanal_mit_geschlossenem_fader() {
        // Der eigentliche Zweck des Vorhörens.
        let mut engine = Engine::new(RATE);
        let a = engine.add_channel("A", ton(220.0, 0.4));
        engine.channel(a).set_fader(0.0);
        engine.channel(a).set_cue(true);

        let out = rendern(&mut engine, 4);

        assert!(
            master_rms(&out, 4) < 0.001,
            "der Kanal steht trotz geschlossenem Fader auf dem Master"
        );
        assert!(
            cue_rms(&out, 4) > 0.1,
            "im Kopfhörer ist nichts zu hören: {:.4}",
            cue_rms(&out, 4)
        );
    }

    #[test]
    fn ohne_cue_taste_bleibt_der_kopfhoerer_still() {
        let mut engine = Engine::new(RATE);
        let a = engine.add_channel("A", ton(220.0, 0.4));
        engine.channel(a).set_fader(1.0);

        let out = rendern(&mut engine, 4);
        assert!(master_rms(&out, 4) > 0.1);
        assert!(cue_rms(&out, 4) < 0.001, "Cue-Bus leckt");
    }

    #[test]
    fn cue_mix_holt_den_master_in_den_kopfhoerer() {
        let mut engine = Engine::new(RATE);
        let a = engine.add_channel("A", ton(220.0, 0.4));
        engine.channel(a).set_fader(1.0);
        engine.set_cue_mix(1.0);

        let out = rendern(&mut engine, 4);
        assert!(
            cue_rms(&out, 4) > 0.1,
            "bei vollem Cue-Mix kommt der Master nicht durch"
        );
    }

    #[test]
    fn zweikanalgeraet_liefert_master_ohne_zu_paniken() {
        let mut engine = Engine::new(RATE);
        let a = engine.add_channel("A", ton(220.0, 0.4));
        engine.channel(a).set_fader(1.0);
        engine.channel(a).set_cue(true);

        let out = rendern(&mut engine, 2);
        assert!(master_rms(&out, 2) > 0.1);
    }

    #[test]
    fn die_summe_wird_begrenzt() {
        let mut engine = Engine::new(RATE);
        for i in 0..4 {
            let ch = engine.add_channel(format!("D{i}"), ton(220.0, 0.9));
            engine.channel(ch).set_fader(1.0);
        }

        let out = rendern(&mut engine, 4);
        let spitze = peak(&out);

        assert!(
            spitze <= engine.limiter().ceiling() + 1e-6,
            "vier laute Kanäle erzeugen Spitze {spitze:.3}"
        );
    }

    #[test]
    fn stille_kanaele_kosten_nichts_und_stoeren_nicht() {
        let mut engine = Engine::new(RATE);
        engine.add_channel("leer", Box::new(SilentSource));
        let a = engine.add_channel("A", ton(220.0, 0.3));
        engine.channel(a).set_fader(1.0);

        let out = rendern(&mut engine, 4);
        assert!(master_rms(&out, 4) > 0.1);
    }

    /// Der Grund, warum die Effekte hinter dem Fader sitzen.
    ///
    /// Zieht man den Fader zu, während ein Delay klingt, muss die Fahne
    /// ausklingen. Ohne die Nachfrage bei [`Channel::klingt_nach`] würde der
    /// Mixer den stummen Kanal überspringen und den Hall abschneiden — die
    /// Post-Fader-Anordnung wäre dann nur auf dem Papier richtig.
    #[test]
    fn die_delayfahne_ueberlebt_den_zugezogenen_fader() {
        use crate::effects::Effekt;

        let mut engine = Engine::new(RATE);
        let kanal = engine.add_channel(
            "A",
            Box::new(LoopSource::new(sine_stereo(440.0, RATE as u32, 0.02))),
        );
        engine.channel(kanal).set_assign(Assign::Thru);
        engine.channel(kanal).set_fader(1.0);

        {
            let fx = engine.channel(kanal).fx();
            fx.set_effekt(Effekt::Delay);
            fx.set_mix(1.0);
            fx.set_amount(0.8);
            fx.set_zeit(0.15);
        }

        // Erst füttern, damit die Leitung voll ist.
        let mut block = vec![0.0f32; 4_096];
        for _ in 0..20 {
            engine.render(&mut block, 2);
        }

        // Jetzt zuziehen — und trotzdem muss etwas herauskommen.
        engine.channel(kanal).set_fader(0.0);

        let mut lautester = 0.0f32;
        for _ in 0..6 {
            block.fill(0.0);
            engine.render(&mut block, 2);
            lautester = lautester.max(block.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        }

        assert!(
            lautester > 0.001,
            "die Fahne reißt beim Zuziehen ab: Restpegel {lautester}"
        );
    }

    /// Die Gegenprobe: Ohne Effekt bleibt ein zugezogener Kanal still.
    #[test]
    fn ohne_effekt_bleibt_der_zugezogene_kanal_still() {
        let mut engine = Engine::new(RATE);
        let kanal = engine.add_channel(
            "A",
            Box::new(LoopSource::new(sine_stereo(440.0, RATE as u32, 0.02))),
        );
        engine.channel(kanal).set_assign(Assign::Thru);
        engine.channel(kanal).set_fader(0.0);

        let mut block = vec![0.0f32; 2_048];
        engine.render(&mut block, 2);
        assert!(rms(&block) < 1e-6, "stummer Kanal ist nicht still");
    }

    /// Der Kopfhörer greift **vor** den Effekten ab.
    ///
    /// Man bereitet den nächsten Track im Kopfhörer vor; ein Effekt, den man
    /// für die Anlage eingestellt hat, gehört dort nicht hinein.
    #[test]
    fn der_kopfhoerer_hoert_die_effekte_nicht() {
        use crate::effects::Effekt;

        // Zwei gleich aufgebaute Mixer, gleich weit gerendert — sonst
        // vergliche man zwei verschiedene Stellen derselben Schleife, und der
        // Unterschied käme vom Signal statt vom Effekt.
        fn bauen(mit_effekt: bool) -> f32 {
            let mut engine = Engine::new(RATE);
            let kanal = engine.add_channel(
                "A",
                Box::new(LoopSource::new(sine_stereo(440.0, RATE as u32, 0.05))),
            );
            engine.channel(kanal).set_cue(true);
            engine.channel(kanal).set_fader(0.0);

            if mit_effekt {
                let fx = engine.channel(kanal).fx();
                fx.set_effekt(Effekt::Gater);
                fx.set_mix(1.0);
                // Fast immer zu — wirkte das auf den Cue-Bus, wäre es nicht
                // zu übersehen.
                fx.set_amount(0.0);
                fx.set_zeit(0.01);
            }

            let mut block = vec![0.0f32; 4 * 1_024];
            let mut summe = 0.0;
            for _ in 0..8 {
                block.fill(0.0);
                engine.render(&mut block, 4);
                summe += rms_channel(&block, 2, 4);
            }
            summe / 8.0
        }

        let ohne = bauen(false);
        let mit = bauen(true);

        assert!(ohne > 0.05, "der Cue-Bus ist von vornherein still: {ohne}");
        assert!(
            (ohne - mit).abs() / ohne < 0.01,
            "der Gater wirkt bis in den Kopfhörer: {ohne:.4} gegen {mit:.4}"
        );
    }
}
