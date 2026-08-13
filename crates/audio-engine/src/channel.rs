//! Ein Kanalzug.
//!
//! Reihenfolge der Kette:
//!
//! ```text
//! Quelle → Trim → EQ → Filter →┬→ Cue-Abgriff (pre-fader)
//!                              └→ Fader → FX → Crossfader → Summe
//! ```
//!
//! **Der Cue-Abgriff liegt vor dem Fader, aber hinter EQ und Filter.** Beides
//! hat einen Grund und beides ist so auf jedem DJ-Mixer verdrahtet:
//!
//! - *Vor dem Fader*, weil man einen Track im Kopfhörer vorbereitet, während
//!   sein Fader unten ist. Läge der Abgriff dahinter, hörte man genau dann
//!   nichts, wenn man ihn braucht.
//! - *Hinter EQ und Filter*, weil man seine Klangeingriffe kontrollieren
//!   können muss, bevor sie auf die Anlage gehen.
//!
//! **Die Effekte liegen hinter dem Fader.** Zieht man ihn zu, während ein
//! Delay klingt, soll die Fahne ausklingen statt abzureißen — genau dafür
//! sitzen Mixer-FX auf jedem Gerät an dieser Stelle. Im Kopfhörer hört man sie
//! deshalb nicht: Der Cue-Abgriff liegt davor.

use crate::crossfader::Assign;
use crate::effects::{Effekt, FxUnit};
use crate::eq::ThreeBandEq;
use crate::filter::DjFilter;
use crate::source::Source;

pub struct Channel {
    pub name: String,
    source: Box<dyn Source>,
    eq: ThreeBandEq,
    filter: DjFilter,
    fx: FxUnit,
    trim: f32,
    fader: f32,
    cue: bool,
    assign: Assign,
    buffer: Vec<f32>,
}

impl Channel {
    pub fn new(name: impl Into<String>, source: Box<dyn Source>, sample_rate: f32) -> Self {
        Channel {
            name: name.into(),
            source,
            eq: ThreeBandEq::new(sample_rate),
            filter: DjFilter::new(sample_rate),
            fx: FxUnit::new(sample_rate),
            trim: 1.0,
            fader: 0.0,
            cue: false,
            assign: Assign::Thru,
            buffer: Vec::new(),
        }
    }

    pub fn set_source(&mut self, source: Box<dyn Source>) -> Box<dyn Source> {
        self.eq.reset();
        self.filter.reset();
        // Der neue Track soll nicht die Hallfahne des alten erben.
        self.fx.reset();
        std::mem::replace(&mut self.source, source)
    }

    pub fn fx(&mut self) -> &mut FxUnit {
        &mut self.fx
    }

    pub fn fx_effekt(&self) -> Effekt {
        self.fx.effekt()
    }

    /// Ob der Kanal noch etwas zu sagen hat, obwohl er stumm ist.
    ///
    /// Der Mixer überspringt stumme Kanäle. Ohne diese Frage risse er jedem
    /// Delay die Fahne ab, sobald der Fader unten ist — und damit genau das
    /// weg, wofür die Effekte hinter dem Fader sitzen.
    pub fn klingt_nach(&self) -> bool {
        self.fx.klingt_nach()
    }

    /// Eingangsverstärkung vor dem EQ.
    pub fn set_trim(&mut self, trim: f32) {
        self.trim = trim.clamp(0.0, 4.0);
    }

    pub fn trim(&self) -> f32 {
        self.trim
    }

    /// Kanalfader, 0.0 bis 1.0.
    pub fn set_fader(&mut self, fader: f32) {
        self.fader = fader.clamp(0.0, 1.0);
    }

    pub fn fader(&self) -> f32 {
        self.fader
    }

    pub fn set_eq(&mut self, low: f32, mid: f32, high: f32) {
        self.eq.set_gains(low, mid, high);
    }

    pub fn eq_gains(&self) -> (f32, f32, f32) {
        self.eq.gains()
    }

    pub fn set_filter(&mut self, position: f32) {
        self.filter.set_position(position);
    }

    pub fn filter_position(&self) -> f32 {
        self.filter.position()
    }

    /// Legt den Kanal auf den Kopfhörer.
    pub fn set_cue(&mut self, cue: bool) {
        self.cue = cue;
    }

    pub fn is_cued(&self) -> bool {
        self.cue
    }

    pub fn set_assign(&mut self, assign: Assign) {
        self.assign = assign;
    }

    pub fn assign(&self) -> Assign {
        self.assign
    }

    /// Rendert die Kette bis einschließlich Filter und gibt das Ergebnis
    /// zurück — also genau das Signal, das der Cue-Bus abgreift.
    ///
    /// Fader und Crossfader liegen bewusst *nicht* darin; die legt der Mixer
    /// beim Summieren an.
    pub fn render_pre_fader(&mut self, frames: usize) -> &[f32] {
        let len = frames * 2;
        if self.buffer.len() < len {
            self.buffer.resize(len, 0.0);
        }
        let buffer = &mut self.buffer[..len];

        self.source.render(buffer);

        if (self.trim - 1.0).abs() > f32::EPSILON {
            for sample in buffer.iter_mut() {
                *sample *= self.trim;
            }
        }

        self.eq.process(buffer);
        self.filter.process(buffer);

        &self.buffer[..len]
    }

    /// Legt Fader und Effekte an — in dieser Reihenfolge.
    ///
    /// Getrennt von [`Channel::render_pre_fader`], weil der Cue-Bus das
    /// Signal davor abgreift und der Mixer es sonst zweimal puffern müsste.
    pub fn process_post_fader(&mut self, buffer: &mut [f32]) {
        if (self.fader - 1.0).abs() > f32::EPSILON {
            for sample in buffer.iter_mut() {
                *sample *= self.fader;
            }
        }
        self.fx.process(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::LoopSource;
    use crate::testing::{rms, sine_stereo};

    const RATE: f32 = 48_000.0;

    fn kanal_mit_ton(freq: f32) -> Channel {
        let source = LoopSource::new(sine_stereo(freq, RATE as u32, 1.0));
        Channel::new("test", Box::new(source), RATE)
    }

    #[test]
    fn fader_startet_unten() {
        // Ein Kanal, der beim Anlegen auf die Anlage geht, ist ein Unfall.
        let kanal = kanal_mit_ton(440.0);
        assert_eq!(kanal.fader(), 0.0);
        assert!(!kanal.is_cued());
        assert_eq!(kanal.assign(), Assign::Thru);
    }

    #[test]
    fn trim_wirkt_vor_dem_eq() {
        let mut kanal = kanal_mit_ton(440.0);
        let voll = rms(kanal.render_pre_fader(4_800)).max(1e-9);

        kanal.set_trim(0.5);
        let halb = rms(kanal.render_pre_fader(4_800));

        assert!(
            (halb / voll - 0.5).abs() < 0.05,
            "Trim 0.5 ergibt Faktor {:.3}",
            halb / voll
        );
    }

    #[test]
    fn der_fader_steckt_nicht_im_pre_fader_signal() {
        let mut kanal = kanal_mit_ton(440.0);
        kanal.set_fader(0.0);

        let pegel = rms(kanal.render_pre_fader(4_800));
        assert!(
            pegel > 0.1,
            "Fader unten dämpft schon das Cue-Signal: {pegel:.4}"
        );
    }

    #[test]
    fn eq_wirkt_auf_das_pre_fader_signal() {
        let mut kanal = kanal_mit_ton(60.0);
        let voll = rms(kanal.render_pre_fader(4_800)).max(1e-9);

        kanal.set_eq(0.0, 1.0, 1.0);
        let gekillt = rms(kanal.render_pre_fader(4_800));

        assert!(
            gekillt / voll < 0.1,
            "Bass-Kill wirkt nicht: Faktor {:.3}",
            gekillt / voll
        );
    }

    #[test]
    fn quellenwechsel_gibt_die_alte_quelle_zurueck() {
        let mut kanal = kanal_mit_ton(440.0);
        let alt = kanal.set_source(Box::new(LoopSource::new(vec![0.0; 4])));

        // Die alte Quelle darf hier fallen, nicht im Audio-Thread.
        drop(alt);
        assert!(rms(kanal.render_pre_fader(128)) < 1e-6);
    }

    #[test]
    fn werte_werden_geklemmt() {
        let mut kanal = kanal_mit_ton(440.0);
        kanal.set_fader(5.0);
        assert_eq!(kanal.fader(), 1.0);
        kanal.set_fader(-1.0);
        assert_eq!(kanal.fader(), 0.0);
        kanal.set_trim(-3.0);
        assert_eq!(kanal.trim(), 0.0);
    }
}
