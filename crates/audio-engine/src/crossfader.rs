//! Crossfader mit einstellbarer Kurve.
//!
//! Kanäle liegen auf Seite A, Seite B oder auf *Thru* — letztere sind vom
//! Crossfader unberührt. Thru ist kein Sonderfall, sondern der Normalfall für
//! alles, was nicht gemischt, sondern dazugelegt wird: ein Mikrofon, ein
//! AUX-Zuspieler, eine Drum-Machine.

/// Wo ein Kanal am Crossfader hängt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Assign {
    A,
    B,
    /// Unberührt vom Crossfader.
    #[default]
    Thru,
}

/// Kurvenform von weich (0.0) bis schneidend (1.0).
///
/// Weich ist eine Konstant-Leistungs-Kurve: In der Mitte liegen beide Seiten
/// bei etwa 0,707, sodass die Summe gleich laut bleibt. Schneidend hält beide
/// Seiten lange auf voll und blendet erst kurz vor dem Anschlag — das ist die
/// Einstellung zum Scratchen.
#[derive(Debug, Clone, Copy)]
pub struct Crossfader {
    position: f32,
    curve: f32,
    gain_a: f32,
    gain_b: f32,
}

impl Default for Crossfader {
    fn default() -> Self {
        Self::new()
    }
}

impl Crossfader {
    pub fn new() -> Self {
        let mut xf = Crossfader {
            position: 0.0,
            curve: 0.0,
            gain_a: 0.0,
            gain_b: 0.0,
        };
        xf.recompute();
        xf
    }

    /// −1 = ganz auf A, 0 = Mitte, +1 = ganz auf B.
    pub fn set_position(&mut self, position: f32) {
        self.position = position.clamp(-1.0, 1.0);
        self.recompute();
    }

    pub fn position(&self) -> f32 {
        self.position
    }

    /// 0.0 = weich (Konstant-Leistung), 1.0 = schneidend.
    pub fn set_curve(&mut self, curve: f32) {
        self.curve = curve.clamp(0.0, 1.0);
        self.recompute();
    }

    pub fn gain(&self, assign: Assign) -> f32 {
        match assign {
            Assign::A => self.gain_a,
            Assign::B => self.gain_b,
            Assign::Thru => 1.0,
        }
    }

    fn recompute(&mut self) {
        let t = (self.position + 1.0) * 0.5;
        let angle = t * std::f32::consts::FRAC_PI_2;

        // Exponent < 1 drückt die Kurve nach oben: beide Seiten bleiben länger
        // laut, der Übergang wird kürzer und härter.
        let exponent = 1.0 - 0.85 * self.curve;

        self.gain_a = angle.cos().clamp(0.0, 1.0).powf(exponent);
        self.gain_b = angle.sin().clamp(0.0, 1.0).powf(exponent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anschlaege_isolieren_je_eine_seite() {
        let mut xf = Crossfader::new();

        xf.set_position(-1.0);
        assert!((xf.gain(Assign::A) - 1.0).abs() < 1e-5);
        assert!(xf.gain(Assign::B) < 1e-5);

        xf.set_position(1.0);
        assert!(xf.gain(Assign::A) < 1e-5);
        assert!((xf.gain(Assign::B) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn mitte_haelt_die_leistung_konstant() {
        let mut xf = Crossfader::new();
        xf.set_curve(0.0);
        xf.set_position(0.0);

        let a = xf.gain(Assign::A);
        let b = xf.gain(Assign::B);

        assert!((a - 0.707).abs() < 0.01, "A in der Mitte: {a:.3}");
        assert!((b - 0.707).abs() < 0.01, "B in der Mitte: {b:.3}");
        assert!(
            (a * a + b * b - 1.0).abs() < 0.01,
            "Leistungssumme weicht ab: {:.3}",
            a * a + b * b
        );
    }

    #[test]
    fn thru_ignoriert_den_crossfader() {
        let mut xf = Crossfader::new();
        for pos in [-1.0, -0.3, 0.0, 0.5, 1.0] {
            xf.set_position(pos);
            assert_eq!(xf.gain(Assign::Thru), 1.0, "bei Position {pos}");
        }
    }

    #[test]
    fn schneidende_kurve_haelt_laenger_voll() {
        let mut weich = Crossfader::new();
        weich.set_curve(0.0);
        weich.set_position(-0.5);

        let mut scharf = Crossfader::new();
        scharf.set_curve(1.0);
        scharf.set_position(-0.5);

        assert!(
            scharf.gain(Assign::A) > weich.gain(Assign::A),
            "schneidend {:.3} sollte über weich {:.3} liegen",
            scharf.gain(Assign::A),
            weich.gain(Assign::A)
        );
    }

    #[test]
    fn position_wird_geklemmt() {
        let mut xf = Crossfader::new();
        xf.set_position(-5.0);
        assert_eq!(xf.position(), -1.0);
        xf.set_position(5.0);
        assert_eq!(xf.position(), 1.0);
    }
}
