//! Wo im Mitschnitt ein Übergang liegt — allein aus dem Klang.
//!
//! Das ist die Schätzung, gegen die alles andere gehalten wird. Sie weiß
//! nichts von Plänen, Griffen oder Reglern; sie hat nur die Datei. Genau
//! deshalb ist sie interessant: Was sie findet, hätte auch ein Zuhörer finden
//! können, und was sie danebenliegt, ist das Maß dafür, wie viel eine
//! Mitschrift wert ist.
//!
//! Sie stand lange im Kritiker selbst. Hier steht sie, weil sie zwei Aufrufer
//! hat: den Kritiker, der einen einzelnen Mitschnitt bewertet, und die
//! Streuung, die dieselbe Schätzung über viele Sets gegen die Wahrheit der
//! Anlage hält. Zwei Kopien wären zwei Verfahren, und die Streuung hätte
//! nichts mehr über den Kritiker gesagt.

use audio_core::track::Track;

use crate::tonart;

/// Fensterlänge für das Klangbild. Eine Sekunde ist kurz genug, um den Beginn
/// einer Blende auf den Takt genau einzugrenzen, und lang genug, dass ein
/// einzelner Schlag das Bild nicht kippt.
pub const FENSTER_SEK: f64 = 1.0;

/// Wie weit zurück verglichen wird. Eine Blende dauert typisch 16 bis 32 Beats,
/// bei 128 BPM also 8 bis 15 Sekunden — der Abstand muss größer sein als das,
/// sonst vergleicht man die Blende mit sich selbst.
pub const ABSTAND_SEK: f64 = 16.0;

/// Ab welcher Änderung des Klangbilds ein Übergang beginnt. 0 heißt gleich,
/// 1 heißt völlig anders. An echten Mitschnitten liegt der Ruhepegel unter
/// 0,1; eine Blende reißt deutlich darüber.
pub const WECHSEL_SCHWELLE: f32 = 0.25;

/// Anteil des Ausschlags, ab dem eine Blende als begonnen gilt.
///
/// Die Schwelle allein taugt nicht als Beginn: Sie reißt erst, wenn der neue
/// Track schon deutlich zu hören ist — bei einer Blende über 16 Sekunden also
/// zehn Sekunden zu spät. Gemessen an einem Mix, dessen Übergang bei genau
/// 40 Sekunden anfing, meldete die Schwelle 51. Deshalb wird vom Ausschlag aus
/// zurückgegangen, bis die Kurve wieder nahe an ihrer Ruhelage liegt.
pub const ANSTIEG_ANTEIL: f32 = 0.12;

/// Ein gefundener Übergang, in Sekunden.
pub struct Uebergang {
    pub beginn: f64,
    pub ende: f64,
    pub hoehe: f32,
    /// Wie unscharf der Beginn ist, in Sekunden.
    ///
    /// Am Anfang einer langen Blende ist der eingehende Track **per
    /// Konstruktion** unhörbar — das ist, was eine Blende ausmacht. Aus dem
    /// Mitschnitt allein lässt sich der Griff an den Fader deshalb nicht auf
    /// den Beat genau zurückverfolgen. Die Spanne vom zurückverfolgten Anstieg
    /// bis zum deutlichen Ausschlag ist das ehrliche Maß dafür.
    pub unschaerfe: f64,
}

/// Findet die Übergänge in einem Mitschnitt.
///
/// Der bequeme Weg: Kurve rechnen und Bereiche suchen in einem Schritt.
pub fn finden(track: &Track) -> Vec<Uebergang> {
    uebergaenge_finden(&wechselkurve(track), FENSTER_SEK)
}

/// Wie stark sich das Klangbild gegenüber vor [`ABSTAND_SEK`] geändert hat.
///
/// Chroma statt Spektrum: Ein Wechsel des Tracks ändert die Harmonik, ein
/// Filterschwenk oder ein Break nicht. Damit findet die Kurve Trackwechsel und
/// nicht jede Veränderung.
pub fn wechselkurve(track: &Track) -> Vec<f32> {
    let rate = track.sample_rate as usize;
    let fenster = (FENSTER_SEK * rate as f64) as usize;
    let abstand = (ABSTAND_SEK / FENSTER_SEK) as usize;

    let chromas: Vec<Option<[f32; 12]>> = track
        .samples
        .chunks(fenster * 2)
        .map(|teil| tonart::chroma(teil, track.sample_rate))
        .collect();

    chromas
        .iter()
        .enumerate()
        .map(|(i, jetzt)| {
            if i < abstand {
                return 0.0;
            }
            match (jetzt, &chromas[i - abstand]) {
                (Some(a), Some(b)) => 1.0 - aehnlichkeit(a, b),
                _ => 0.0,
            }
        })
        .collect()
}

/// Kosinus-Ähnlichkeit zweier Chroma-Vektoren, 0 bis 1.
fn aehnlichkeit(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let punkt: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na <= f32::EPSILON || nb <= f32::EPSILON {
        return 1.0;
    }
    (punkt / (na * nb)).clamp(0.0, 1.0)
}

/// Zusammenhängende Bereiche über der Schwelle, mit zurückverfolgtem Beginn.
///
/// Der Beginn ist die Stelle, an der die Kurve **anfängt zu steigen** — nicht
/// die, an der sie die Schwelle reißt. Auf den ersten kommt es an, wenn man
/// nach der Phrasenlage fragt: Dort hat der Bediener den Fader angefasst.
pub fn uebergaenge_finden(kurve: &[f32], sek_je_wert: f64) -> Vec<Uebergang> {
    let mut aus = Vec::new();
    let mut start: Option<usize> = None;
    let mut hoehe = 0.0f32;

    let abschliessen = |aus: &mut Vec<Uebergang>, s: usize, e: usize, hoehe: f32| {
        let ruhe = ruhelage(kurve);
        let grenze = ruhe + (hoehe - ruhe) * ANSTIEG_ANTEIL;
        let mut beginn = s;
        while beginn > 0 && kurve[beginn - 1] > grenze {
            beginn -= 1;
        }
        aus.push(Uebergang {
            // Der Vergleich blickt zurück, also liegt der hörbare Beginn eine
            // Fensterbreite vor dem gemessenen Anstieg.
            beginn: (beginn as f64 - 1.0).max(0.0) * sek_je_wert,
            ende: e as f64 * sek_je_wert,
            hoehe,
            unschaerfe: (s - beginn) as f64 * sek_je_wert,
        });
    };

    for (i, &w) in kurve.iter().enumerate() {
        if w >= WECHSEL_SCHWELLE {
            start.get_or_insert(i);
            hoehe = hoehe.max(w);
        } else if let Some(s) = start.take() {
            abschliessen(&mut aus, s, i, hoehe);
            hoehe = 0.0;
        }
    }
    if let Some(s) = start {
        abschliessen(&mut aus, s, kurve.len(), hoehe);
    }
    aus
}

/// Ruhelage der Wechselkurve: der Median.
///
/// Nicht der Mittelwert — die Ausschläge selbst würden ihn anheben und den
/// Beginn damit systematisch zu spät setzen.
fn ruhelage(kurve: &[f32]) -> f32 {
    let mut sortiert: Vec<f32> = kurve.iter().copied().filter(|w| w.is_finite()).collect();
    if sortiert.is_empty() {
        return 0.0;
    }
    sortiert.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sortiert[sortiert.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chroma_mit(spitzen: &[usize]) -> [f32; 12] {
        let mut c = [0.05f32; 12];
        for &s in spitzen {
            c[s] = 1.0;
        }
        c
    }

    #[test]
    fn gleiches_klangbild_ist_kein_wechsel() {
        let a = chroma_mit(&[0, 4, 7]);
        assert!((aehnlichkeit(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ein_anderer_akkord_faellt_auf() {
        let a = chroma_mit(&[0, 4, 7]);
        let b = chroma_mit(&[1, 5, 8]);
        assert!(
            aehnlichkeit(&a, &b) < 0.4,
            "zwei fremde Akkorde galten als ähnlich: {}",
            aehnlichkeit(&a, &b)
        );
    }

    /// Ein stiller Abschnitt darf keinen Wechsel vortäuschen.
    #[test]
    fn stille_gilt_als_unveraendert() {
        let a = chroma_mit(&[0, 4, 7]);
        let leer = [0.0f32; 12];
        assert_eq!(aehnlichkeit(&a, &leer), 1.0);
    }

    #[test]
    fn ein_zusammenhaengender_ausschlag_ist_ein_uebergang() {
        // Ruhe, dann ein Ausschlag über vier Sekunden, dann wieder Ruhe.
        let mut kurve = vec![0.05; 30];
        for w in kurve.iter_mut().skip(10).take(4) {
            *w = 0.6;
        }
        let gefunden = uebergaenge_finden(&kurve, 1.0);

        assert_eq!(gefunden.len(), 1);
        assert!(
            (gefunden[0].beginn - 9.0).abs() < 0.01,
            "{}",
            gefunden[0].beginn
        );
        assert!((gefunden[0].hoehe - 0.6).abs() < 1e-6);
    }

    /// Der eigentliche Punkt: Eine Blende steigt langsam an, und die Schwelle
    /// reißt erst spät. Gefragt ist, wo der Anstieg **anfing**.
    #[test]
    fn der_beginn_wird_bis_zum_anstieg_zurueckverfolgt() {
        let mut kurve = vec![0.02; 40];
        // Ab Sekunde 10 steigt es langsam, ab 20 reißt es die Schwelle.
        for (n, i) in (10..25).enumerate() {
            kurve[i] = 0.02 + 0.05 * n as f32;
        }

        let gefunden = uebergaenge_finden(&kurve, 1.0);
        assert_eq!(gefunden.len(), 1);
        assert!(
            gefunden[0].beginn <= 11.0,
            "der Beginn wurde nicht zurückverfolgt: {}",
            gefunden[0].beginn
        );
    }

    #[test]
    fn die_ruhelage_ist_der_median_und_nicht_der_mittelwert() {
        // Ein einzelner großer Ausschlag zieht den Mittelwert hoch, den Median
        // nicht — und ein zu hoher Ruhewert setzt jeden Beginn zu spät.
        let mut kurve = vec![0.02; 100];
        kurve[50] = 0.9;
        assert!((ruhelage(&kurve) - 0.02).abs() < 1e-6);
    }

    #[test]
    fn zwei_getrennte_ausschlaege_sind_zwei_uebergaenge() {
        let mut kurve = vec![0.05; 60];
        for i in [10, 11, 12, 40, 41, 42] {
            kurve[i] = 0.5;
        }
        assert_eq!(uebergaenge_finden(&kurve, 1.0).len(), 2);
    }

    /// Ein Ausschlag, der bis zum Ende reicht, geht nicht verloren.
    #[test]
    fn ein_ausschlag_am_ende_zaehlt_mit() {
        let mut kurve = vec![0.05; 20];
        for w in kurve.iter_mut().skip(17) {
            *w = 0.7;
        }
        assert_eq!(uebergaenge_finden(&kurve, 1.0).len(), 1);
    }

    #[test]
    fn eine_ruhige_kurve_meldet_nichts() {
        // Kein Ausschlag heißt kein Übergang. Ein erfundener wäre schlimmer
        // als ein übersehener: Er bekäme in der Streuung eine Abweichung.
        let kurve = vec![0.03f32; 60];
        assert!(uebergaenge_finden(&kurve, 1.0).is_empty());
    }
}
