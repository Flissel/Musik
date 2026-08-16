//! Was von außen hereinkommt — die Reaktion des Raums.
//!
//! Ein DJ liest die Fläche: Wird es voller, geht die Energie hoch, kippt die
//! Stimmung? Ein Agent kann das nicht sehen. Damit er trotzdem darauf reagieren
//! kann, muss es jemand hereingeben — ein Mikrofonpegel, eine Umfrage auf dem
//! Handy, ein Mensch, der im Chat „wird voller" tippt.
//!
//! **Ein einzelner Wert nützt dabei fast nichts.** „Energie 0,7" beantwortet
//! keine Frage; „0,7 und seit zwei Minuten fallend" beantwortet sie. Deshalb
//! merkt sich ein Signal seine jüngste Vergangenheit und rechnet daraus den
//! Trend aus — dieselbe Überlegung wie bei `beats_left`: Was ein Bediener sonst
//! bei jedem Blick selbst ausrechnet, rechnet er irgendwann falsch.
//!
//! Vier feste Plätze statt beliebig vieler Namen. Ein Platz trägt seine
//! Bedeutung als Text, so wie man am Mischpult einen Kanalzug mit Klebeband
//! beschriftet. Das hält den Katalog statisch — die Alternative wären zur
//! Laufzeit geleakte Zeichenketten, und dieselbe Entscheidung ist schon bei den
//! Hot Cues so gefallen.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Wie viele Signale es gibt.
pub const SIGNALE: usize = 4;

/// Über welchen Zeitraum der Trend gerechnet wird.
///
/// Zwei Minuten sind an einer Tanzfläche eine Aussage: kürzer misst man das
/// Rauschen einzelner Momente, länger verpasst man den Umschwung, auf den man
/// reagieren wollte.
pub const FENSTER: Duration = Duration::from_secs(120);

/// Wie viele Proben höchstens aufbewahrt werden.
///
/// Deckel gegen einen Sender, der im Millisekundentakt schickt — die Liste soll
/// nicht wachsen, nur weil jemand zu eifrig meldet.
pub const MAX_PROBEN: usize = 240;

/// Ein Wert von außen, mit seiner jüngsten Vergangenheit.
#[derive(Debug, Clone, Default)]
pub struct Signal {
    /// Was der Wert bedeutet. Leer heißt: noch nie benutzt.
    pub name: String,
    proben: VecDeque<(Instant, f64)>,
}

impl Signal {
    pub fn neu() -> Signal {
        Signal::default()
    }

    /// Trägt einen Messwert ein.
    ///
    /// Die Zeit kommt von außen, damit sich das hier prüfen lässt, ohne zu
    /// warten.
    pub fn setzen(&mut self, wert: f64, jetzt: Instant) {
        self.proben.push_back((jetzt, wert));
        self.aufraeumen(jetzt);
    }

    fn aufraeumen(&mut self, jetzt: Instant) {
        while self
            .proben
            .front()
            .is_some_and(|(t, _)| jetzt.duration_since(*t) > FENSTER)
        {
            self.proben.pop_front();
        }
        while self.proben.len() > MAX_PROBEN {
            self.proben.pop_front();
        }
    }

    /// Der zuletzt gemeldete Wert.
    ///
    /// Bleibt stehen, auch wenn lange nichts kam — es ist die letzte bekannte
    /// Lage, und die verschwindet nicht dadurch, dass niemand mehr misst. Wie
    /// alt sie ist, sagt [`Signal::alter`]; wer stillschweigend nicht auf alte
    /// Werte hereinfallen will, fragt danach.
    pub fn wert(&self) -> Option<f64> {
        self.proben.back().map(|(_, w)| *w)
    }

    /// Sekunden seit der letzten Meldung.
    pub fn alter(&self, jetzt: Instant) -> Option<f64> {
        self.proben
            .back()
            .map(|(t, _)| jetzt.saturating_duration_since(*t).as_secs_f64())
    }

    /// Änderung je Minute, als Steigung einer Ausgleichsgeraden.
    ///
    /// Nicht einfach „letzter minus erster": Ein einzelner Ausreißer am Rand
    /// würde daraus eine Behauptung machen, die die übrigen Proben nicht
    /// hergeben. `None`, solange weniger als zwei Proben im Fenster liegen —
    /// aus einem Punkt lässt sich keine Richtung ablesen.
    pub fn trend(&self, jetzt: Instant) -> Option<f64> {
        let gueltig: Vec<(f64, f64)> = self
            .proben
            .iter()
            .filter(|(t, _)| jetzt.duration_since(*t) <= FENSTER)
            .map(|(t, w)| (jetzt.duration_since(*t).as_secs_f64(), *w))
            .collect();

        if gueltig.len() < 2 {
            return None;
        }

        // x läuft rückwärts (Sekunden *her*), also am Ende das Vorzeichen
        // drehen: Ein steigendes Signal soll einen positiven Trend haben.
        let n = gueltig.len() as f64;
        let x_mittel = gueltig.iter().map(|(x, _)| x).sum::<f64>() / n;
        let y_mittel = gueltig.iter().map(|(_, y)| y).sum::<f64>() / n;

        let mut oben = 0.0;
        let mut unten = 0.0;
        for (x, y) in &gueltig {
            oben += (x - x_mittel) * (y - y_mittel);
            unten += (x - x_mittel) * (x - x_mittel);
        }
        if unten <= f64::EPSILON {
            // Alle Proben zur selben Zeit — keine Steigung, aber auch kein
            // Fehler.
            return Some(0.0);
        }

        Some(-oben / unten * 60.0)
    }

    pub fn ist_leer(&self) -> bool {
        self.proben.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nach(start: Instant, sekunden: u64) -> Instant {
        start + Duration::from_secs(sekunden)
    }

    #[test]
    fn ein_steigendes_signal_hat_einen_positiven_trend() {
        let start = Instant::now();
        let mut s = Signal::neu();
        // Von 0,2 auf 0,6 in einer Minute.
        for (sek, wert) in [(0, 0.2), (20, 0.33), (40, 0.47), (60, 0.6)] {
            s.setzen(wert, nach(start, sek));
        }

        let trend = s.trend(nach(start, 60)).expect("kein Trend");
        assert!(
            (trend - 0.4).abs() < 0.02,
            "rund 0,4 je Minute erwartet, war {trend:.3}"
        );
        assert_eq!(s.wert(), Some(0.6));
    }

    #[test]
    fn ein_fallendes_signal_hat_einen_negativen_trend() {
        let start = Instant::now();
        let mut s = Signal::neu();
        for (sek, wert) in [(0, 0.9), (30, 0.6), (60, 0.3)] {
            s.setzen(wert, nach(start, sek));
        }
        assert!(s.trend(nach(start, 60)).unwrap() < -0.5);
    }

    /// Ein Ausreißer am Rand darf die Richtung nicht umdrehen.
    ///
    /// Genau dafür die Ausgleichsgerade und nicht „letzter minus erster": Eine
    /// einzelne Fehlmessung würde daraus eine Behauptung machen, die die
    /// übrigen Proben nicht hergeben.
    #[test]
    fn ein_ausreisser_dreht_die_richtung_nicht_um() {
        let start = Instant::now();
        let mut s = Signal::neu();
        for (sek, wert) in [
            (0, 0.2),
            (10, 0.3),
            (20, 0.4),
            (30, 0.5),
            (40, 0.6),
            (50, 0.7),
            (60, 0.1),
        ] {
            s.setzen(wert, nach(start, sek));
        }
        assert!(
            s.trend(nach(start, 60)).unwrap() > 0.0,
            "der letzte Wert allein hat die Richtung bestimmt"
        );
    }

    #[test]
    fn aus_einer_probe_laesst_sich_keine_richtung_lesen() {
        let start = Instant::now();
        let mut s = Signal::neu();
        assert_eq!(s.trend(start), None);
        s.setzen(0.5, start);
        assert_eq!(s.trend(start), None, "eine Probe ist keine Richtung");
        assert_eq!(s.wert(), Some(0.5));
    }

    /// Was aus dem Fenster gefallen ist, zählt für den Trend nicht mehr — der
    /// Wert selbst bleibt aber die letzte bekannte Lage.
    #[test]
    fn alte_proben_fallen_aus_dem_fenster_der_wert_bleibt() {
        let start = Instant::now();
        let mut s = Signal::neu();
        s.setzen(0.2, start);
        s.setzen(0.8, nach(start, 10));

        let spaeter = nach(start, 400);
        assert_eq!(s.wert(), Some(0.8), "die letzte bekannte Lage fehlt");
        assert_eq!(s.trend(spaeter), None, "uralte Proben ergaben einen Trend");
        assert!(s.alter(spaeter).unwrap() > 380.0);
    }

    #[test]
    fn die_probenliste_waechst_nicht_unbegrenzt() {
        let start = Instant::now();
        let mut s = Signal::neu();
        // Ein eifriger Sender: tausend Meldungen in derselben Sekunde.
        for _ in 0..1_000 {
            s.setzen(0.5, start);
        }
        assert!(s.proben.len() <= MAX_PROBEN, "{}", s.proben.len());
    }
}
