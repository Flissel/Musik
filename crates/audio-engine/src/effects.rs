//! Effekte für den Kanalzug.
//!
//! Vier statt vierzig. Traktor hat über 40, und die meisten benutzt niemand;
//! was ein DJ wirklich in den Fingern hat, ist ein tempo-synchrones Delay, ein
//! Gate, ein Flanger und etwas Schmutz. Lieber vier, die stimmen, als vierzig,
//! die man einmal ausprobiert.
//!
//! **Die Effekte liegen post-fader.** Das ist keine Kleinigkeit: Zieht man den
//! Fader zu, während ein Delay noch klingt, soll die Fahne ausklingen und
//! nicht abreißen. Genau dafür haben Mixer ihre Mixer-FX hinter dem Fader
//! sitzen, und genau deshalb muss der Mixer einen Kanal mit klingendem Effekt
//! weiterrechnen, auch wenn er stumm ist — siehe [`FxUnit::klingt_nach`].
//!
//! Echtzeit: Alle Puffer werden beim Anlegen belegt, nie im Callback. Die
//! Länge des Delays ist deshalb nach oben begrenzt; [`MAX_DELAY_SEKUNDEN`]
//! sagt, wie weit.

/// Längste einstellbare Verzögerung. Bei 60 BPM ist ein ganzer Takt vier
/// Sekunden — mehr als das braucht im Kanalzug niemand, und der Puffer kostet
/// Speicher, der beim Anlegen belegt sein muss.
pub const MAX_DELAY_SEKUNDEN: f32 = 4.0;

/// Wie lange das Gate zum Öffnen und Schließen braucht.
///
/// Ohne diese Rampe schaltet es hart von 1 auf 0 und knackt bei jedem
/// Durchgang — ein Sprung im Signal ist ein Klick, egal wie sauber der Rest
/// ist.
const GATE_RAMPE_MS: f32 = 1.5;

/// Ab welchem Restpegel ein Effekt als ausgeklungen gilt.
const STILLE: f32 = 1e-5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effekt {
    Aus,
    /// Tempo-synchrones Echo mit Rückkopplung.
    Delay,
    /// Rhythmisches Tor.
    Gater,
    /// Kammfilter mit wanderndem Zahn.
    Flanger,
    /// Bit- und Ratenreduktion.
    Crush,
}

impl Effekt {
    pub fn name(&self) -> &'static str {
        match self {
            Effekt::Aus => "off",
            Effekt::Delay => "delay",
            Effekt::Gater => "gater",
            Effekt::Flanger => "flanger",
            Effekt::Crush => "crush",
        }
    }

    pub fn aus_name(name: &str) -> Option<Effekt> {
        Some(match name {
            "off" | "aus" => Effekt::Aus,
            "delay" => Effekt::Delay,
            "gater" => Effekt::Gater,
            "flanger" => Effekt::Flanger,
            "crush" => Effekt::Crush,
            _ => return None,
        })
    }

    /// Alle Namen, für den Katalog.
    pub const NAMEN: &'static [&'static str] = &["off", "delay", "gater", "flanger", "crush"];
}

/// Eine Verzögerungsleitung fester Länge.
struct Leitung {
    puffer: Vec<f32>,
    schreib: usize,
}

impl Leitung {
    fn neu(frames: usize) -> Leitung {
        Leitung {
            // Stereo, verschränkt.
            puffer: vec![0.0; frames.max(1) * 2],
            schreib: 0,
        }
    }

    fn leeren(&mut self) {
        self.puffer.fill(0.0);
        self.schreib = 0;
    }

    fn frames(&self) -> usize {
        self.puffer.len() / 2
    }

    /// Liest mit gebrochener Verzögerung, linear zwischen zwei Frames.
    ///
    /// Ohne die Zwischenwerte hörte man beim Flanger jede Stufe der
    /// wandernden Verzögerung als Zwitschern.
    fn lies(&self, verzoegerung: f32, kanal: usize) -> f32 {
        let frames = self.frames();
        let d = verzoegerung.clamp(1.0, frames as f32 - 2.0);
        let ganz = d.floor();
        let bruch = d - ganz;

        let a = (self.schreib + frames - ganz as usize) % frames;
        let b = (a + frames - 1) % frames;
        self.puffer[a * 2 + kanal] * (1.0 - bruch) + self.puffer[b * 2 + kanal] * bruch
    }

    fn schreibe(&mut self, links: f32, rechts: f32) {
        self.puffer[self.schreib * 2] = links;
        self.puffer[self.schreib * 2 + 1] = rechts;
        self.schreib = (self.schreib + 1) % self.frames();
    }
}

/// Ein Effektgerät im Kanalzug.
pub struct FxUnit {
    sample_rate: f32,
    effekt: Effekt,
    /// Trocken/nass, 0 bis 1.
    mix: f32,
    /// Bedeutung hängt am Effekt: Rückkopplung, Öffnungsdauer, Tiefe, Härte.
    amount: f32,
    /// Zeit in Sekunden: Delay-Länge, Gate-Periode, Flanger-Umlauf.
    zeit: f32,

    delay: Leitung,
    flanger: Leitung,
    /// Phase des Gaters und des Flanger-LFO, 0 bis 1.
    phase: f32,
    /// Zuletzt ausgegebener Gate-Faktor, damit die Rampe stetig bleibt.
    gate: f32,
    /// Sample-and-Hold für den Crusher.
    halt: [f32; 2],
    halt_zaehler: f32,
    /// Grobes Maß dafür, wie laut der Effekt zuletzt noch war.
    rest: f32,
}

impl FxUnit {
    pub fn new(sample_rate: f32) -> FxUnit {
        let max = (sample_rate * MAX_DELAY_SEKUNDEN) as usize;
        FxUnit {
            sample_rate,
            effekt: Effekt::Aus,
            mix: 0.0,
            amount: 0.5,
            zeit: 0.5,
            delay: Leitung::neu(max),
            // Der Flanger braucht Millisekunden, keine Sekunden.
            flanger: Leitung::neu((sample_rate * 0.03) as usize),
            phase: 0.0,
            gate: 1.0,
            halt: [0.0; 2],
            halt_zaehler: 0.0,
            rest: 0.0,
        }
    }

    pub fn set_effekt(&mut self, effekt: Effekt) {
        if effekt == self.effekt {
            return;
        }
        self.effekt = effekt;
        // Ein Wechsel schleppt den alten Klang sonst mit hinüber.
        self.reset();
    }

    pub fn effekt(&self) -> Effekt {
        self.effekt
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn mix(&self) -> f32 {
        self.mix
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    pub fn amount(&self) -> f32 {
        self.amount
    }

    /// Zeit in Sekunden.
    pub fn set_zeit(&mut self, sekunden: f32) {
        self.zeit = sekunden.clamp(0.001, MAX_DELAY_SEKUNDEN);
    }

    pub fn zeit(&self) -> f32 {
        self.zeit
    }

    pub fn reset(&mut self) {
        self.delay.leeren();
        self.flanger.leeren();
        self.phase = 0.0;
        self.gate = 1.0;
        self.halt = [0.0; 2];
        self.halt_zaehler = 0.0;
        self.rest = 0.0;
    }

    /// Ob der Effekt noch klingt, obwohl nichts mehr hineingeht.
    ///
    /// Der Mixer überspringt stumme Kanäle, um Rechenzeit zu sparen. Genau das
    /// würde die Fahne eines Delays abschneiden, sobald man den Fader zuzieht —
    /// also fragt er hier nach, bevor er überspringt.
    pub fn klingt_nach(&self) -> bool {
        matches!(self.effekt, Effekt::Delay | Effekt::Flanger)
            && self.mix > 0.0
            && self.rest > STILLE
    }

    /// Verarbeitet einen Block verschränktes Stereo, an Ort und Stelle.
    pub fn process(&mut self, buffer: &mut [f32]) {
        if self.effekt == Effekt::Aus || self.mix <= 0.0 {
            // Trocken heißt wirklich trocken: keine Rundung, kein Rechnen.
            return;
        }

        match self.effekt {
            Effekt::Delay => self.delay_process(buffer),
            Effekt::Gater => self.gater_process(buffer),
            Effekt::Flanger => self.flanger_process(buffer),
            Effekt::Crush => self.crush_process(buffer),
            Effekt::Aus => {}
        }
    }

    fn delay_process(&mut self, buffer: &mut [f32]) {
        let verzoegerung =
            (self.zeit * self.sample_rate).clamp(1.0, self.delay.frames() as f32 - 2.0);
        // Bis 0.9: Bei 1.0 wächst die Rückkopplung ohne Ende, und ein Delay,
        // das sich aufschaukelt, ist auf einer Anlage gefährlich.
        let rueck = self.amount * 0.9;
        let mut spitze = 0.0f32;

        for frame in buffer.chunks_exact_mut(2) {
            let nass_l = self.delay.lies(verzoegerung, 0);
            let nass_r = self.delay.lies(verzoegerung, 1);

            self.delay
                .schreibe(frame[0] + nass_l * rueck, frame[1] + nass_r * rueck);

            frame[0] = frame[0] * (1.0 - self.mix) + nass_l * self.mix;
            frame[1] = frame[1] * (1.0 - self.mix) + nass_r * self.mix;

            spitze = spitze.max(nass_l.abs()).max(nass_r.abs());
        }

        self.rest = spitze;
    }

    fn gater_process(&mut self, buffer: &mut [f32]) {
        let periode = (self.zeit * self.sample_rate).max(2.0);
        let schritt = 1.0 / periode;
        // Öffnungsdauer von einem Viertel bis fast ganz — ein Gate, das immer
        // offen ist, tut nichts, eines das kaum öffnet, ist nur ein Knacken.
        let offen = 0.15 + self.amount * 0.7;
        let rampe = (GATE_RAMPE_MS * 0.001 * self.sample_rate).max(1.0);
        let pro_sample = 1.0 / rampe;

        for frame in buffer.chunks_exact_mut(2) {
            let ziel = if self.phase < offen { 1.0 } else { 0.0 };
            // Stetig auf das Ziel zulaufen statt springen.
            if self.gate < ziel {
                self.gate = (self.gate + pro_sample).min(ziel);
            } else if self.gate > ziel {
                self.gate = (self.gate - pro_sample).max(ziel);
            }

            let g = 1.0 - self.mix + self.gate * self.mix;
            frame[0] *= g;
            frame[1] *= g;

            self.phase += schritt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }

        // Ein Gate klingt nicht nach: Es dämpft nur, was hineingeht.
        self.rest = 0.0;
    }

    fn flanger_process(&mut self, buffer: &mut [f32]) {
        // Ein bis zehn Millisekunden — darüber wird aus dem Flanger ein Chorus.
        let mitte = 0.005 * self.sample_rate;
        let tiefe = 0.004 * self.sample_rate * self.amount;
        let schritt = 1.0 / (self.zeit * self.sample_rate).max(2.0);
        let rueck = 0.4 * self.amount;
        let mut spitze = 0.0f32;

        for frame in buffer.chunks_exact_mut(2) {
            let lfo = (std::f32::consts::TAU * self.phase).sin();
            let verzoegerung = mitte + tiefe * lfo;

            let nass_l = self.flanger.lies(verzoegerung, 0);
            let nass_r = self.flanger.lies(verzoegerung, 1);

            self.flanger
                .schreibe(frame[0] + nass_l * rueck, frame[1] + nass_r * rueck);

            frame[0] = frame[0] * (1.0 - self.mix) + nass_l * self.mix;
            frame[1] = frame[1] * (1.0 - self.mix) + nass_r * self.mix;

            self.phase += schritt;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            spitze = spitze.max(nass_l.abs()).max(nass_r.abs());
        }

        self.rest = spitze;
    }

    fn crush_process(&mut self, buffer: &mut [f32]) {
        // Zwölf Bit hinunter bis vier — darunter bleibt nur noch Krach.
        let bits = 12.0 - self.amount * 8.0;
        let stufen = (2.0f32).powf(bits);
        // Und gleichzeitig die Rate herunter, bis auf ein Achtel.
        let halten = 1.0 + self.amount * 7.0;

        for frame in buffer.chunks_exact_mut(2) {
            self.halt_zaehler += 1.0;
            if self.halt_zaehler >= halten {
                self.halt_zaehler -= halten;
                self.halt[0] = (frame[0] * stufen).round() / stufen;
                self.halt[1] = (frame[1] * stufen).round() / stufen;
            }

            frame[0] = frame[0] * (1.0 - self.mix) + self.halt[0] * self.mix;
            frame[1] = frame[1] * (1.0 - self.mix) + self.halt[1] * self.mix;
        }

        self.rest = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn sinus(freq: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (std::f32::consts::TAU * freq * i as f32 / RATE).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    fn impuls(frames: usize) -> Vec<f32> {
        let mut v = vec![0.0; frames * 2];
        v[0] = 1.0;
        v[1] = 1.0;
        v
    }

    fn spitze(buffer: &[f32]) -> f32 {
        buffer.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    fn rms(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|v| v * v).sum::<f32>() / buffer.len().max(1) as f32).sqrt()
    }

    /// Der wichtigste Test überhaupt: Trocken muss trocken heißen.
    #[test]
    fn bei_mix_null_bleibt_das_signal_unberuehrt() {
        for effekt in [Effekt::Delay, Effekt::Gater, Effekt::Flanger, Effekt::Crush] {
            let original = sinus(440.0, 2_000);
            let mut buffer = original.clone();

            let mut fx = FxUnit::new(RATE);
            fx.set_effekt(effekt);
            fx.set_mix(0.0);
            fx.process(&mut buffer);

            assert_eq!(
                buffer,
                original,
                "{} verändert das Signal bei Mix 0",
                effekt.name()
            );
        }
    }

    #[test]
    fn ausgeschaltet_wird_gar_nicht_gerechnet() {
        let original = sinus(440.0, 500);
        let mut buffer = original.clone();

        let mut fx = FxUnit::new(RATE);
        fx.set_mix(1.0);
        fx.process(&mut buffer);

        assert_eq!(buffer, original);
    }

    #[test]
    fn das_delay_wiederholt_zur_richtigen_zeit() {
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Delay);
        fx.set_mix(1.0);
        fx.set_amount(0.0);
        fx.set_zeit(0.1);

        let mut buffer = impuls(RATE as usize);
        fx.process(&mut buffer);

        // Die Wiederholung muss auf 0.1 s liegen — ein Delay, das die Zeit
        // nicht trifft, ist beim Mixen unbrauchbar.
        let erwartet = (0.1 * RATE) as usize;
        let gefunden = (0..RATE as usize)
            .max_by(|a, b| {
                buffer[a * 2]
                    .abs()
                    .partial_cmp(&buffer[b * 2].abs())
                    .unwrap()
            })
            .unwrap();

        assert!(
            (gefunden as i64 - erwartet as i64).abs() <= 2,
            "Wiederholung bei {gefunden} statt {erwartet}"
        );
    }

    #[test]
    fn die_rueckkopplung_schaukelt_sich_nicht_auf() {
        // Bei voller Rückkopplung darf das Delay lauter werden, aber nicht
        // über alle Grenzen — ein Delay, das explodiert, ist auf einer
        // Anlage gefährlich.
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Delay);
        fx.set_mix(1.0);
        fx.set_amount(1.0);
        fx.set_zeit(0.05);

        let mut hoechster = 0.0f32;
        for durchgang in 0..40 {
            let mut buffer = if durchgang == 0 {
                impuls(RATE as usize / 2)
            } else {
                vec![0.0; RATE as usize]
            };
            fx.process(&mut buffer);
            hoechster = hoechster.max(spitze(&buffer));
            assert!(
                buffer.iter().all(|v| v.is_finite()),
                "Durchgang {durchgang} liefert kein endliches Signal"
            );
        }

        assert!(hoechster < 4.0, "Rückkopplung wächst auf {hoechster}");
    }

    #[test]
    fn das_delay_klingt_nach_wenn_nichts_mehr_hineingeht() {
        // Das ist die Voraussetzung dafür, dass der Mixer den Kanal
        // weiterrechnet, wenn der Fader zu ist.
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Delay);
        fx.set_mix(1.0);
        fx.set_amount(0.7);
        fx.set_zeit(0.05);

        let mut buffer = impuls(RATE as usize / 4);
        fx.process(&mut buffer);
        assert!(fx.klingt_nach(), "das Delay meldet sich nicht als klingend");

        // Ein Gate dagegen klingt nie nach — es dämpft nur.
        let mut gate = FxUnit::new(RATE);
        gate.set_effekt(Effekt::Gater);
        gate.set_mix(1.0);
        let mut b = sinus(440.0, 1_000);
        gate.process(&mut b);
        assert!(!gate.klingt_nach());
    }

    #[test]
    fn das_tor_schliesst_und_oeffnet_ohne_zu_knacken() {
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Gater);
        fx.set_mix(1.0);
        fx.set_amount(0.5);
        fx.set_zeit(0.1);

        // Gleichspannung, damit jeder Sprung im Ausgang vom Tor kommt und
        // nicht vom Signal.
        let mut buffer = vec![0.5; RATE as usize * 2];
        fx.process(&mut buffer);

        let groesster_sprung = buffer
            .chunks_exact(2)
            .zip(buffer.chunks_exact(2).skip(1))
            .fold(0.0f32, |m, (a, b)| m.max((b[0] - a[0]).abs()));

        // Über eine Rampe von 1,5 ms darf ein Schritt nur ein Bruchteil sein.
        assert!(
            groesster_sprung < 0.02,
            "das Tor springt um {groesster_sprung} — das knackt"
        );
    }

    #[test]
    fn das_tor_macht_wirklich_leiser() {
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Gater);
        fx.set_mix(1.0);
        fx.set_amount(0.0);
        fx.set_zeit(0.05);

        let original = sinus(440.0, RATE as usize);
        let mut buffer = original.clone();
        fx.process(&mut buffer);

        let verhaeltnis = rms(&buffer) / rms(&original);
        // Bei kleinstem Amount ist das Tor 15 % der Zeit offen.
        assert!(
            verhaeltnis < 0.55,
            "Tor dämpft kaum: Faktor {verhaeltnis:.3}"
        );
    }

    #[test]
    fn der_crusher_reduziert_die_stufen() {
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Crush);
        fx.set_mix(1.0);
        fx.set_amount(1.0);

        let mut buffer = sinus(220.0, 4_000);
        fx.process(&mut buffer);

        let mut werte: Vec<i64> = buffer.iter().map(|v| (v * 1e6) as i64).collect();
        werte.sort_unstable();
        werte.dedup();

        // Vier Bit über eine halbe Amplitude sind eine Handvoll Stufen.
        assert!(
            werte.len() < 40,
            "{} verschiedene Werte — da wird nichts reduziert",
            werte.len()
        );
    }

    #[test]
    fn der_flanger_faerbt_ohne_zu_zerstoeren() {
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Flanger);
        fx.set_mix(0.5);
        fx.set_amount(0.8);
        fx.set_zeit(2.0);

        let original = sinus(440.0, RATE as usize);
        let mut buffer = original.clone();
        fx.process(&mut buffer);

        assert!(buffer.iter().all(|v| v.is_finite()));
        assert_ne!(buffer, original, "der Flanger tut nichts");
        // Ein Flanger hebt und senkt, aber er darf nicht übersteuern.
        assert!(
            spitze(&buffer) < 1.5,
            "Flanger übersteuert: {}",
            spitze(&buffer)
        );
    }

    #[test]
    fn ein_wechsel_schleppt_den_alten_klang_nicht_mit() {
        let mut fx = FxUnit::new(RATE);
        fx.set_effekt(Effekt::Delay);
        fx.set_mix(1.0);
        fx.set_amount(0.8);
        fx.set_zeit(0.2);

        let mut buffer = impuls(RATE as usize / 4);
        fx.process(&mut buffer);
        assert!(fx.klingt_nach());

        fx.set_effekt(Effekt::Flanger);
        assert!(
            !fx.klingt_nach(),
            "der alte Hall steckt noch im neuen Effekt"
        );
    }

    #[test]
    fn namen_gehen_hin_und_zurueck() {
        for name in Effekt::NAMEN {
            let e = Effekt::aus_name(name).unwrap_or_else(|| panic!("{name} unbekannt"));
            assert_eq!(e.name(), *name);
        }
        assert_eq!(Effekt::aus_name("hall"), None);
    }
}
