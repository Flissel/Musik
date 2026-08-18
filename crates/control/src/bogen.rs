//! Der Bogen: was das Set vorhat, bevor der nächste Track gewählt wird.
//!
//! Ein einzelner guter Übergang ist Handwerk. Ein gutes Set ist Architektur:
//! Aufbau, Plateau, Bruch, Wiederaufbau — über eine Stunde, nicht über vier
//! Minuten. Bisher konnte die Anlage jeden Übergang begründen und keine
//! Reihenfolge.
//!
//! **Für ein Team ist der Bogen das, worüber man sich einig sein muss.** Ohne
//! ihn verhandeln zwei Agenten über den nächsten Track ohne gemeinsamen
//! Maßstab: Der eine will Druck, der andere Luft, und beide haben recht, weil
//! es keinen Satz gibt, gegen den sich das prüfen ließe. Mit ihm heißt die
//! Frage nicht mehr „welcher Track ist gut", sondern „was fehlt hier gerade" —
//! und darauf gibt es eine Zahl.
//!
//! ```text
//! set master.arc 0 0.3, 20 0.7, 45 0.95, 60 0.5, 80 1.0
//! do master.arc_start
//! get master.arc_gap      → value master.arc_gap 0.250000
//! ```
//!
//! Die Zeiten sind **Minuten** — ein Set dauert Stunden, keine Sekunden. Wer
//! genauer will, schreibt `m:ss`; `20:30` sind zwanzigeinhalb Minuten.
//!
//! # Woher die Ist-Energie kommt — und woher nicht
//!
//! **Nicht aus dem Pegel.** Die Gliederung misst je Abschnitt einen Pegel, aber
//! der ist auf den lautesten Abschnitt *desselben* Tracks bezogen: Der Drop
//! eines leisen Stücks steht dort genauso bei 0,99 wie der eines lauten. Über
//! Tracks hinweg ist das nicht vergleichbar, und ein Bogen, der solche Zahlen
//! addiert, rechnet mit Äpfeln.
//!
//! Stattdessen aus der **Art** des laufenden Abschnitts. Ein Drop ist ein Drop,
//! ob leise produziert oder laut. Das ist grob — sechs Stufen, mehr nicht — und
//! grob ist hier ehrlicher als eine Nachkommastelle, die niemand einlösen kann.
//! Wer es feiner braucht, misst den Raum und gibt ihn als Signal herein.

use audio_core::Art;

/// Wie viel Energie eine Abschnittsart bedeutet.
///
/// Eine grobe Leiter, keine Messung. Sie beantwortet die einzige Frage, die
/// sich über Tracks hinweg stellen lässt: Ist das hier gerade ein Höhepunkt,
/// eine Atempause oder etwas dazwischen?
pub fn energie(art: Art) -> f64 {
    match art {
        Art::Intro => 0.2,
        Art::Aufbau => 0.55,
        Art::Drop => 0.9,
        // Ein Break ist nicht so leer wie ein Intro: Die Drums laufen meist
        // weiter, und der Raum bleibt wach.
        Art::Break => 0.35,
        Art::Outro => 0.2,
        Art::Teil => 0.6,
    }
}

/// Ein Stützpunkt: zu dieser Minute so viel Energie.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Punkt {
    pub minute: f64,
    pub energie: f64,
}

/// Die Ziel-Energiekurve über die Setdauer.
///
/// Zwischen den Stützpunkten wird geradlinig verbunden — nicht weil das
/// musikalisch stimmt, sondern weil alles andere eine Genauigkeit vortäuschte,
/// die eine von Hand gesetzte Kurve nicht hat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Bogen {
    punkte: Vec<Punkt>,
}

impl Bogen {
    pub fn neu() -> Bogen {
        Bogen::default()
    }

    pub fn ist_leer(&self) -> bool {
        self.punkte.is_empty()
    }

    pub fn punkte(&self) -> &[Punkt] {
        &self.punkte
    }

    /// Wie lang der Bogen ist, in Minuten.
    pub fn dauer(&self) -> f64 {
        self.punkte.last().map(|p| p.minute).unwrap_or(0.0)
    }

    /// Was der Bogen zu dieser Minute vorsieht.
    ///
    /// Vor dem ersten Punkt gilt der erste, nach dem letzten der letzte: Ein
    /// Set, das länger läuft als geplant, soll nicht plötzlich auf null fallen.
    pub fn soll(&self, minute: f64) -> Option<f64> {
        let erster = self.punkte.first()?;
        let letzter = self.punkte.last()?;
        if minute <= erster.minute {
            return Some(erster.energie);
        }
        if minute >= letzter.minute {
            return Some(letzter.energie);
        }

        let paar = self
            .punkte
            .windows(2)
            .find(|w| minute >= w[0].minute && minute <= w[1].minute)?;
        let (a, b) = (paar[0], paar[1]);
        let spanne = b.minute - a.minute;
        if spanne <= 0.0 {
            return Some(b.energie);
        }
        let t = (minute - a.minute) / spanne;
        Some(a.energie + (b.energie - a.energie) * t)
    }

    /// Wohin es von hier aus geht, über die nächsten `voraus` Minuten.
    pub fn verlauf(&self, minute: f64, voraus: f64) -> Option<Verlauf> {
        let jetzt = self.soll(minute)?;
        let gleich = self.soll(minute + voraus)?;
        Some(Verlauf::aus_differenz(gleich - jetzt))
    }
}

/// Wohin der Bogen als Nächstes will.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verlauf {
    Steigt,
    Haelt,
    Faellt,
}

impl Verlauf {
    /// Ab welcher Änderung eine Richtung eine ist.
    ///
    /// Unter einem Zwanzigstel ist es ein Plateau — und ein Plateau ist eine
    /// Aussage, keine Verlegenheit: „hier bleibt es, wo es ist" beantwortet die
    /// Frage nach dem nächsten Track genauso wie „hier geht es hoch".
    pub const SCHWELLE: f64 = 0.05;

    pub fn aus_differenz(d: f64) -> Verlauf {
        if d > Verlauf::SCHWELLE {
            Verlauf::Steigt
        } else if d < -Verlauf::SCHWELLE {
            Verlauf::Faellt
        } else {
            Verlauf::Haelt
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Verlauf::Steigt => "steigt",
            Verlauf::Haelt => "haelt",
            Verlauf::Faellt => "faellt",
        }
    }
}

/// Liest einen Bogen aus Text: `0 0.3, 20 0.7, 60 0.5`.
///
/// Zeiten in **Minuten**, wahlweise als `m:ss`. Die Punkte werden sortiert — eine Zeile,
/// in der sie durcheinandergeraten sind, würde sonst still falsch
/// interpoliert.
pub fn parse(text: &str) -> Result<Bogen, String> {
    let mut punkte = Vec::new();
    for stueck in text.split(',') {
        let stueck = stueck.trim();
        if stueck.is_empty() {
            continue;
        }
        let (zeit, energie) = stueck
            .split_once(char::is_whitespace)
            .ok_or_else(|| format!("{stueck} ist kein Punkt: '<zeit> <energie>' erwartet"))?;

        let minute = zeit_lesen(zeit.trim())?;
        let energie: f64 = energie
            .trim()
            .parse()
            .map_err(|_| format!("{energie} ist keine Energie zwischen 0 und 1"))?;
        if !(0.0..=1.0).contains(&energie) {
            return Err(format!("{energie} liegt nicht zwischen 0 und 1"));
        }
        if minute < 0.0 {
            return Err(format!("{zeit} liegt vor dem Anfang"));
        }
        punkte.push(Punkt { minute, energie });
    }

    if punkte.len() < 2 {
        return Err("ein Bogen braucht mindestens zwei Punkte".into());
    }
    punkte.sort_by(|a, b| a.minute.total_cmp(&b.minute));
    Ok(Bogen { punkte })
}

fn zeit_lesen(text: &str) -> Result<f64, String> {
    match text.split_once(':') {
        Some((min, sek)) => {
            let min: f64 = min.parse().map_err(|_| format!("{text} ist keine Zeit"))?;
            let sek: f64 = sek.parse().map_err(|_| format!("{text} ist keine Zeit"))?;
            Ok(min + sek / 60.0)
        }
        None => text.parse().map_err(|_| format!("{text} ist keine Zeit")),
    }
}

/// Schreibt einen Bogen so, wie `parse` ihn wieder liest.
pub fn text(bogen: &Bogen) -> String {
    bogen
        .punkte
        .iter()
        .map(|p| {
            let minuten = p.minute.floor();
            let sekunden = ((p.minute - minuten) * 60.0).round();
            format!("{minuten:.0}:{sekunden:02.0} {:.2}", p.energie)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beispiel() -> Bogen {
        parse("0 0.3, 20 0.7, 45 0.95, 60 0.5").expect("lesbar")
    }

    #[test]
    fn ein_bogen_wird_gelesen_und_wieder_geschrieben() {
        let b = beispiel();
        assert_eq!(b.punkte().len(), 4);
        assert_eq!(b.dauer(), 60.0);
        assert_eq!(text(&b), "0:00 0.30, 20:00 0.70, 45:00 0.95, 60:00 0.50");
        // Und `m:ss` wird genauso gelesen: zwanzigeinhalb Minuten.
        assert_eq!(parse("0 0.3, 20:30 0.7").unwrap().dauer(), 20.5);
        assert_eq!(parse(&text(&b)).unwrap(), b);
    }

    #[test]
    fn zwischen_den_punkten_wird_geradlinig_verbunden() {
        let b = beispiel();
        assert_eq!(b.soll(0.0), Some(0.3));
        assert_eq!(b.soll(20.0), Some(0.7));
        // Auf halbem Weg zwischen 0 und 20.
        assert!((b.soll(10.0).unwrap() - 0.5).abs() < 1e-9);
    }

    /// Ein Set, das länger läuft als geplant, fällt nicht plötzlich auf null.
    #[test]
    fn vor_dem_anfang_und_nach_dem_ende_gilt_der_rand() {
        let b = beispiel();
        assert_eq!(b.soll(-5.0), Some(0.3));
        assert_eq!(b.soll(999.0), Some(0.5));
    }

    #[test]
    fn der_verlauf_sagt_wohin_es_geht() {
        let b = beispiel();
        assert_eq!(b.verlauf(0.0, 5.0), Some(Verlauf::Steigt));
        assert_eq!(b.verlauf(50.0, 5.0), Some(Verlauf::Faellt));
        // Am Ende geht es nirgendwohin mehr.
        assert_eq!(b.verlauf(90.0, 5.0), Some(Verlauf::Haelt));
    }

    /// Ein Plateau ist eine Aussage, keine Verlegenheit.
    #[test]
    fn ein_flacher_abschnitt_haelt() {
        let b = parse("0 0.7, 30 0.72, 60 0.9").unwrap();
        assert_eq!(b.verlauf(5.0, 5.0), Some(Verlauf::Haelt));
    }

    /// Durcheinandergeratene Zeiten würden sonst still falsch interpoliert.
    #[test]
    fn die_punkte_werden_sortiert() {
        let b = parse("60 0.5, 0 0.3, 20 0.7").unwrap();
        assert_eq!(b.punkte()[0].minute, 0.0);
        assert_eq!(b.punkte()[2].minute, 60.0);
    }

    /// Unsinn wird gemeldet, nicht verschluckt — sonst führe ein Set gegen eine
    /// Kurve, die niemand gemeint hat.
    #[test]
    fn unsinn_wird_gemeldet() {
        assert!(parse("").is_err(), "leer");
        assert!(parse("0 0.3").is_err(), "ein Punkt ist kein Bogen");
        assert!(parse("0 0.3, 20 1.5").is_err(), "Energie über 1");
        assert!(parse("0 0.3, 20 -0.1").is_err(), "Energie unter 0");
        assert!(parse("0 0.3, gleich 0.7").is_err(), "keine Zeit");
        assert!(parse("0, 20 0.7").is_err(), "keine Energie");
    }

    #[test]
    fn ein_leerer_bogen_sagt_nichts_statt_null() {
        let b = Bogen::neu();
        assert!(b.ist_leer());
        assert_eq!(b.soll(10.0), None);
        assert_eq!(b.verlauf(10.0, 5.0), None);
    }

    /// Die Leiter der Abschnittsarten: grob, aber über Tracks hinweg
    /// vergleichbar — anders als der Pegel.
    #[test]
    fn die_energieleiter_ordnet_die_abschnitte() {
        assert!(energie(Art::Drop) > energie(Art::Aufbau));
        assert!(energie(Art::Aufbau) > energie(Art::Break));
        assert!(energie(Art::Break) > energie(Art::Intro));
        assert_eq!(energie(Art::Intro), energie(Art::Outro));
        for art in Art::ALLE {
            assert!((0.0..=1.0).contains(&energie(art)), "{}", art.name());
        }
    }
}
