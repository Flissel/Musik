//! Der Raum: was draußen geschieht, verändert, was das Set will.
//!
//! Die vier Signalplätze gab es schon, aber sie waren Deko — Werte gingen
//! hinein, und nichts hat darauf reagiert. Ein Bogen, der von der ersten Minute
//! an feststeht, ist ein Abspielplan; genau das sollte hier nie herauskommen.
//!
//! # Was gebeugt wird und was nicht
//!
//! Der Raum verschiebt **das Ziel**, nicht die Kurve. `master.arc` bleibt, was
//! jemand aufgeschrieben hat; `master.arc_curve` zeigt es unverändert. Gebeugt
//! wird `master.arc_target` — und damit `arc_gap`, die Zahl, nach der gewählt
//! wird. Wer nachsehen will, wie groß der Eingriff ist, liest
//! `master.room_bend`.
//!
//! Das ist der ganze Unterschied zwischen „der Raum redet mit" und „der Raum
//! schreibt um": Am Ende des Abends steht in `arc` immer noch der Plan, gegen
//! den sich vergleichen lässt, was tatsächlich passiert ist.
//!
//! # Der Trend beugt, nicht der Wert
//!
//! Gebeugt wird nach der **Steigung** des Signals, nicht nach seiner Höhe. Der
//! Grund ist, dass die Höhe nichts Vergleichbares ist: Was „0,6 Andrang"
//! bedeutet, weiß nur, wer den Sender geschrieben hat, und eine Anlage, die
//! diese Zahl direkt gegen die Energie des Bogens rechnet, addiert Äpfel — der
//! gleiche Fehler, der beim Ist-Wert des Bogens schon einmal auffiel.
//!
//! Eine Änderung ist dagegen eine Aussage über denselben Sender: „seit drei
//! Minuten fallend" heißt dasselbe, egal wie die Skala gemeint war. Deshalb
//! `wirkung = trend_je_minute * gewicht`, begrenzt auf [`GRENZE`].
//!
//! # Die Grenze
//!
//! Der Raum darf das Ziel um höchstens [`GRENZE`] verschieben. Ohne Deckel
//! könnte ein hängender oder falsch skalierter Sender das Set übernehmen, und
//! das wäre schlimmer als ein Set, das den Raum ignoriert: Ein Bogen, der
//! wegen eines klemmenden Reglers auf 1,0 steht, ist kein Bogen mehr.

/// Wie weit der Raum das Ziel höchstens verschiebt.
///
/// 0,25 ist gut ein Viertel der Energieskala — genug, um aus „Plateau halten"
/// ein „noch etwas drauflegen" zu machen, zu wenig, um aus einem Break einen
/// Drop zu machen. Wer mehr Wirkung will, sagt das über das Gewicht; über die
/// Grenze kommt er trotzdem nicht.
pub const GRENZE: f64 = 0.25;

/// Welches Signal den Bogen beugt und wie stark.
#[derive(Debug, Clone, PartialEq)]
pub struct Raum {
    /// Der Signalplatz, 0-basiert.
    pub platz: usize,
    /// Wieviel Energie eine Änderung von 1,0 je Minute bewirkt.
    pub gewicht: f64,
}

impl Raum {
    /// Liest `signal1 0.3` — Platz und Gewicht.
    ///
    /// Das Gewicht darf fehlen; dann ist es 1,0. Ein Gewicht von 0 ist erlaubt
    /// und heißt: zugehört, aber nicht gefolgt.
    pub fn parse(text: &str, signale: usize) -> Option<Raum> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (name, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let nummer: usize = name.trim().strip_prefix("signal")?.parse().ok()?;
        if nummer == 0 || nummer > signale {
            return None;
        }
        let gewicht: f64 = match rest.trim() {
            "" => 1.0,
            g => g.parse().ok()?,
        };
        if !gewicht.is_finite() || gewicht < 0.0 {
            return None;
        }
        Some(Raum {
            platz: nummer - 1,
            gewicht,
        })
    }

    pub fn text(&self) -> String {
        format!("signal{} {}", self.platz + 1, self.gewicht)
    }

    /// Wie weit das Ziel verschoben wird.
    ///
    /// `None`, solange das Signal keinen Trend hat — aus einem einzelnen Wert
    /// lässt sich keine Richtung ablesen, und eine Beugung um 0 zu behaupten
    /// wäre etwas anderes als zu sagen, dass man nichts weiß.
    pub fn beugung(&self, trend: Option<f64>) -> Option<f64> {
        Some((trend? * self.gewicht).clamp(-GRENZE, GRENZE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ein_raum_liest_sich_aus_platz_und_gewicht() {
        assert_eq!(
            Raum::parse("signal1 0.3", 4),
            Some(Raum {
                platz: 0,
                gewicht: 0.3
            })
        );
        // Ohne Gewicht: volle Wirkung.
        assert_eq!(
            Raum::parse("signal4", 4),
            Some(Raum {
                platz: 3,
                gewicht: 1.0
            })
        );
        assert_eq!(Raum::parse("signal1 0.3", 4).unwrap().text(), "signal1 0.3");
    }

    /// Ein Platz, den es nicht gibt, wird abgewiesen statt stillschweigend auf
    /// einen anderen umgebogen: Ein Bogen, der einem Signal folgt, das niemand
    /// gemeint hat, ist schlimmer als einer ohne Raum.
    #[test]
    fn unsinn_wird_abgewiesen() {
        for text in [
            "",
            "  ",
            "signal0",
            "signal5",
            "signal",
            "energie 1",
            "signal1 -1",
            "signal1 viel",
            "signal1 inf",
        ] {
            assert_eq!(Raum::parse(text, 4), None, "{text:?}");
        }
    }

    #[test]
    fn der_trend_beugt_und_die_grenze_haelt() {
        let r = Raum {
            platz: 0,
            gewicht: 0.5,
        };
        assert_eq!(r.beugung(Some(0.4)), Some(0.2));
        assert_eq!(r.beugung(Some(-0.4)), Some(-0.2));
        // Ohne Trend keine Behauptung.
        assert_eq!(r.beugung(None), None);

        // Ein Sender, der ausrastet, übernimmt das Set nicht.
        assert_eq!(r.beugung(Some(1000.0)), Some(GRENZE));
        assert_eq!(r.beugung(Some(-1000.0)), Some(-GRENZE));
    }

    /// Zugehört, aber nicht gefolgt — erlaubt, und der Unterschied zu „kein
    /// Raum gesetzt" bleibt sichtbar.
    #[test]
    fn ein_gewicht_von_null_ist_erlaubt() {
        let r = Raum::parse("signal2 0", 4).expect("gültig");
        assert_eq!(r.beugung(Some(0.9)), Some(0.0));
    }
}
