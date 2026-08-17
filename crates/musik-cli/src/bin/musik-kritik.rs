//! Der Kritiker: liest einen Mitschnitt und benennt die Handwerksfehler.
//!
//! **Warum das früh kommt und nicht spät.** Ohne ihn bleibt „klingt besser"
//! eine Meinung. Jede Regel im Mixen ist eine Behauptung, bis jemand sie am
//! Ergebnis misst — und in diesem Projekt hat genau diese Wendung schon
//! mehrfach einen Fehler ans Licht geholt, den niemand vermutet hatte.
//!
//! Er läuft **offline über den Mitschnitt**, nicht im Echtzeitpfad. Damit darf
//! er so gründlich sein, wie er will, und er braucht von der Anlage nichts als
//! die WAV-Datei, die sie ohnehin schreibt.
//!
//! Was er misst, misst er ohne Geschmacksurteil:
//!
//! - **Wo ein Übergang liegt** — über die Änderung des Klangbilds, nicht über
//!   ein Protokoll. Was er findet, hat wirklich geklungen.
//! - **Wie lang er dauert** — und wie unscharf sein Beginn ist.
//! - **Was der Pegel dabei macht** — das Loch in der Mitte einer Blende hört
//!   man, bevor man es erklären kann.
//! - **Ob das Tempo durchhält** — vor und nach dem Übergang.
//!
//! **Eine Grenze, die er selbst benennt:** Am Anfang einer langen Blende ist
//! der eingehende Track per Konstruktion unhörbar — das ist, was eine Blende
//! ausmacht. Der Griff an den Fader lässt sich aus dem Mitschnitt allein
//! deshalb nicht auf den Beat genau zurückverfolgen. Der Kritiker gibt die
//! Unschärfe mit an und schweigt zur Phrasenlage, wenn sie größer ist als der
//! gemessene Versatz. Um das zu schließen, müsste er das Plan-Protokoll
//! mitlesen — das ist der nächste Schritt, nicht dieser.
//!
//! Was er ausdrücklich **nicht** tut: eine Note geben. Ob ein Set gut war,
//! entscheidet weiterhin jemand, der dabei war. Der Kritiker nimmt ihm die
//! Handwerksfehler ab, nicht das Urteil.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use analysis::{onset, tempo, tonart};
use audio_core::track::Track;

/// Fensterlänge für das Klangbild. Eine Sekunde ist kurz genug, um den Beginn
/// einer Blende auf den Takt genau einzugrenzen, und lang genug, dass ein
/// einzelner Schlag das Bild nicht kippt.
const FENSTER_SEK: f64 = 1.0;

/// Wie weit zurück verglichen wird. Eine Blende dauert typisch 16 bis 32 Beats,
/// bei 128 BPM also 8 bis 15 Sekunden — der Abstand muss größer sein als das,
/// sonst vergleicht man die Blende mit sich selbst.
const ABSTAND_SEK: f64 = 16.0;

/// Ab welcher Änderung des Klangbilds ein Übergang beginnt. 0 heißt gleich,
/// 1 heißt völlig anders. An echten Mitschnitten liegt der Ruhepegel unter
/// 0,1; eine Blende reißt deutlich darüber.
const WECHSEL_SCHWELLE: f32 = 0.25;

struct Optionen {
    datei: PathBuf,
}

fn main() -> Result<()> {
    let opts = argumente()?;
    let track = Track::decode_file(&opts.datei)
        .with_context(|| format!("{} ließ sich nicht lesen", opts.datei.display()))?;

    println!("Mitschnitt: {}", opts.datei.display());
    println!(
        "  {:.1} s, {} Hz\n",
        track.duration_secs(),
        track.sample_rate
    );

    let kurve = wechselkurve(&track);
    let uebergaenge = uebergaenge_finden(&kurve, FENSTER_SEK);

    pegel_bericht(&track);

    if uebergaenge.is_empty() {
        println!("\nKein Übergang gefunden.");
        println!("  Entweder lief nur ein Track, oder der Wechsel war zu leise für");
        println!("  die Schwelle. Das ist ein Befund, keine Entwarnung.");
        return Ok(());
    }

    println!("\n{} Übergänge gefunden:", uebergaenge.len());
    for (i, u) in uebergaenge.iter().enumerate() {
        println!("\n── Übergang {} ──────────────────────────────", i + 1);
        uebergang_bericht(&track, u);
    }

    Ok(())
}

/// Ein gefundener Übergang, in Sekunden.
struct Uebergang {
    beginn: f64,
    ende: f64,
    hoehe: f32,
    /// Wie unscharf der Beginn ist, in Sekunden.
    ///
    /// Am Anfang einer langen Blende ist der eingehende Track **per
    /// Konstruktion** unhörbar — das ist, was eine Blende ausmacht. Aus dem
    /// Mitschnitt allein lässt sich der Griff an den Fader deshalb nicht auf
    /// den Beat genau zurückverfolgen. Die Spanne vom zurückverfolgten Anstieg
    /// bis zum deutlichen Ausschlag ist das ehrliche Maß dafür.
    unschaerfe: f64,
}

/// Wie stark sich das Klangbild gegenüber vor [`ABSTAND_SEK`] geändert hat.
///
/// Chroma statt Spektrum: Ein Wechsel des Tracks ändert die Harmonik, ein
/// Filterschwenk oder ein Break nicht. Damit findet die Kurve Trackwechsel und
/// nicht jede Veränderung.
fn wechselkurve(track: &Track) -> Vec<f32> {
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

/// Anteil des Ausschlags, ab dem eine Blende als begonnen gilt.
///
/// Die Schwelle allein taugt nicht als Beginn: Sie reißt erst, wenn der neue
/// Track schon deutlich zu hören ist — bei einer Blende über 16 Sekunden also
/// zehn Sekunden zu spät. Gemessen an einem Mix, dessen Übergang bei genau
/// 40 Sekunden anfing, meldete die Schwelle 51. Deshalb wird vom Ausschlag aus
/// zurückgegangen, bis die Kurve wieder nahe an ihrer Ruhelage liegt.
const ANSTIEG_ANTEIL: f32 = 0.12;

/// Zusammenhängende Bereiche über der Schwelle, mit zurückverfolgtem Beginn.
///
/// Der Beginn ist die Stelle, an der die Kurve **anfängt zu steigen** — nicht
/// die, an der sie die Schwelle reißt. Auf den ersten kommt es an, wenn man
/// nach der Phrasenlage fragt: Dort hat der Bediener den Fader angefasst.
fn uebergaenge_finden(kurve: &[f32], sek_je_wert: f64) -> Vec<Uebergang> {
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

/// Pegel über den ganzen Mitschnitt, in Sekundenfenstern.
fn pegel(track: &Track) -> Vec<f32> {
    let fenster = track.sample_rate as usize * 2;
    track
        .samples
        .chunks(fenster)
        .map(|t| {
            if t.is_empty() {
                return 0.0;
            }
            (t.iter().map(|x| x * x).sum::<f32>() / t.len() as f32).sqrt()
        })
        .collect()
}

fn pegel_bericht(track: &Track) {
    let p = pegel(track);
    let laut: Vec<f32> = p.iter().copied().filter(|&x| x > 0.001).collect();
    if laut.is_empty() {
        println!("Pegel: der Mitschnitt ist still.");
        return;
    }
    let mittel = laut.iter().sum::<f32>() / laut.len() as f32;
    let (min, max) = laut
        .iter()
        .fold((f32::MAX, 0.0f32), |(a, b), &x| (a.min(x), b.max(x)));
    println!("Pegel: Mittel {mittel:.3}, von {min:.3} bis {max:.3}");
    if max > 0.99 {
        println!("  ⚠ Der Begrenzer hat gearbeitet — die Summe stand am Anschlag.");
    }
}

fn uebergang_bericht(track: &Track, u: &Uebergang) {
    let dauer = u.ende - u.beginn;
    println!(
        "  Beginn {:>7.2} s (±{:.0} s), Dauer {:.1} s, Stärke {:.2}",
        u.beginn, u.unschaerfe, dauer, u.hoehe
    );

    // --- Phrasenlage ----------------------------------------------------
    // Das Raster kommt aus dem Material *davor*: Dort läuft der ausgehende
    // Track allein, und auf dessen Eins gehört der Übergang.
    match raster_vor(track, u.beginn) {
        Some(grid) => {
            let rate = track.sample_rate;
            // `analysis::Beatgrid` beschreibt das Raster, rechnen kann damit
            // das aus `audio_core` — dieselben Zahlen, andere Aufgabe.
            let raster = audio_core::Beatgrid::new(grid.bpm, grid.anchor_frames, 1.0);
            let beat = raster.beat_at(u.beginn * rate as f64, rate);
            let zum_beat = (beat - beat.round()).abs();
            println!(
                "  Raster davor: {:.2} BPM · Beginn auf Beat {:.2}",
                grid.bpm, beat
            );
            let unschaerfe_beats = u.unschaerfe * grid.bpm as f64 / 60.0;
            println!(
                "  Abstand zum nächsten Schlag: {zum_beat:.2} Beats \
(Unschärfe ±{unschaerfe_beats:.1})"
            );

            // Bewusst kein Urteil über Beat- oder Phrasenlage. Zwei Gründe,
            // beide hart:
            //
            // Das Fenster ist ein Sekunde breit, bei 126 BPM also zwei Beats.
            // Damit lässt sich nicht feststellen, ob ein Einsatz auf dem
            // Schlag sitzt — die Auflösung ist gröber als die Frage.
            //
            // Und der Anker des Detektors ist der erste starke Schlag im
            // Analysefenster, nicht der Anfang einer Phrase. „Sechs Beats
            // neben der Eins" wäre gegen einen willkürlichen Nullpunkt
            // gemessen.
            //
            // Beides ließe sich schließen — feinere Fenster für das eine, ein
            // bekannter Downbeat aus der Strukturanalyse oder dem
            // Plan-Protokoll für das andere. Bis dahin steht hier die Zahl
            // ohne Urteil, und daneben, was fehlt.
            println!("  · Beat- und Phrasenlage sind so nicht beurteilbar: Die Unschärfe");
            println!("     (±{unschaerfe_beats:.0} Beats) ist größer als der Versatz, und der");
            println!("     Nullpunkt des Rasters ist nicht der Anfang einer Phrase.");
        }
        None => println!("  Raster davor nicht erkennbar — Phrasenlage nicht beurteilbar."),
    }

    // --- Pegelloch ------------------------------------------------------
    let p = pegel(track);
    let idx = |s: f64| (s / 2.0) as usize;
    let (a, b) = (idx(u.beginn), idx(u.ende).min(p.len()));
    if b > a && b <= p.len() {
        let innen = p[a..b].iter().copied().fold(f32::MAX, f32::min);
        let davor = p[a.saturating_sub(4)..a]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        let danach = p[b..(b + 4).min(p.len())]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        let aussen = davor.max(danach);
        if aussen > 0.001 {
            let verlust = 100.0 * (1.0 - innen / aussen);
            println!("  Pegel in der Blende: {innen:.3} gegen {aussen:.3} außen");
            if verlust > 15.0 {
                println!("  ⚠ {verlust:.0} % Einbruch in der Mitte. Das ist die Crossfader-Kurve;");
                println!("     `master.crossfader_curve` härter stellen.");
            } else {
                println!("  ✓ Pegel hält über die Blende ({verlust:.0} % Abweichung).");
            }
        }
    }

    // --- Tempo davor und danach -----------------------------------------
    match (raster_vor(track, u.beginn), raster_nach(track, u.ende)) {
        (Some(vor), Some(nach)) => {
            let ab = (nach.bpm - vor.bpm).abs();
            println!("  Tempo: {:.2} → {:.2} BPM", vor.bpm, nach.bpm);
            if ab > 0.5 {
                println!("  ⚠ {ab:.2} BPM Unterschied über den Übergang — beide Decks liefen");
                println!("     nicht auf demselben Tempo, oder der Sync ist zwischendurch weg.");
            } else {
                println!("  ✓ Tempo hält über den Übergang.");
            }
        }
        _ => println!("  Tempo vorher/nachher nicht beidseitig erkennbar."),
    }
}

/// Beatgrid aus dem Material vor einer Stelle.
fn raster_vor(track: &Track, sekunde: f64) -> Option<tempo::Beatgrid> {
    let rate = track.sample_rate as f64;
    let ende = (sekunde * rate) as usize * 2;
    let laenge = (20.0 * rate) as usize * 2;
    let start = ende.saturating_sub(laenge);
    raster_von(track, start, ende)
}

fn raster_nach(track: &Track, sekunde: f64) -> Option<tempo::Beatgrid> {
    let rate = track.sample_rate as f64;
    let start = (sekunde * rate) as usize * 2;
    let ende = (start + (20.0 * rate) as usize * 2).min(track.samples.len());
    raster_von(track, start, ende)
}

/// Erkennt ein Raster in einem Ausschnitt und rechnet seinen Anker auf den
/// ganzen Mitschnitt zurück.
///
/// Ohne das Zurückrechnen läge der Anker relativ zum Ausschnitt, und jede
/// Beat-Angabe wäre um dessen Beginn verschoben.
fn raster_von(track: &Track, start: usize, ende: usize) -> Option<tempo::Beatgrid> {
    if ende <= start || ende - start < track.sample_rate as usize {
        return None;
    }
    let teil = &track.samples[start..ende.min(track.samples.len())];
    let hüll = onset::onset_envelope(teil, track.sample_rate);
    let grid = tempo::detect(&hüll)?;
    let grid = tempo::refine_anchor(teil, track.sample_rate, grid);

    Some(tempo::Beatgrid {
        anchor_frames: grid.anchor_frames + (start / 2) as u64,
        ..grid
    })
}

fn argumente() -> Result<Optionen> {
    let mut datei = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                hilfe();
                std::process::exit(0);
            }
            _ => datei = Some(PathBuf::from(arg)),
        }
    }

    let Some(datei) = datei else {
        hilfe();
        bail!("keine Datei angegeben");
    };
    if !Path::new(&datei).exists() {
        bail!("{} gibt es nicht", datei.display());
    }
    Ok(Optionen { datei })
}

fn hilfe() {
    println!("Aufruf: musik-kritik <mitschnitt.wav>");
    println!();
    println!("Liest einen Mitschnitt und benennt, was messbar ist: wo ein Übergang");
    println!("liegt, wie lang er dauert, was der Pegel dabei macht und ob das Tempo");
    println!("durchhält. Zur Beat- und Phrasenlage schweigt er — dafür ist die");
    println!("Zeitauflösung zu grob und der Nullpunkt des Rasters willkürlich.");
    println!();
    println!("Er gibt keine Note. Ob ein Set gut war, entscheidet, wer dabei war.");
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
}
