//! Eine lange Aufnahme in einzelne Lieder zerlegen.
//!
//! Wer den AUX-Eingang mitlaufen lässt, hat hinterher **eine** Datei mit einer
//! Stunde Musik darin. Die Anlage kann damit wenig anfangen: Ein Beatgrid gilt
//! je Track, eine Tonart auch, und die Gliederung sucht Intro und Outro eines
//! Stücks, nicht eines Abends.
//!
//! Also schneiden. Zwei Wege, und der erste ist der verlässliche:
//!
//! 1. **Die Lücke.** Zwischen zwei Liedern wird es kurz still. Das ist kein
//!    Verfahren, sondern ein Ablesen — wo nichts ist, ist eine Grenze.
//! 2. **Der Klangwechsel.** Wo keine Lücke ist, bleibt der Vergleich der
//!    Harmonik, derselbe, den auch der Kritiker benutzt. Der ist eine
//!    Schätzung, und seine Grenzen stehen unten.
//!
//! # Wo das nicht trägt
//!
//! **Ein gemixtes Set lässt sich nicht sauber schneiden**, und zwar nicht aus
//! Mangel an Verfahren: Während einer Blende laufen zwei Lieder gleichzeitig.
//! Eine Grenze gibt es dort nicht, nur eine Stelle, an der man sie zieht. Der
//! Klangwechsel findet eine Blende über mehr als zwei Phrasen ohnehin nicht —
//! er vergleicht mit dem Klangbild von vor sechzehn Sekunden, und eine längere
//! Blende schließt diesen Vergleich in sich ein (siehe `docs/FAHRPLAN.md`, N3).
//!
//! Für eine Aufnahme von einer Platte, einer Playlist oder einem Stream mit
//! Pausen dazwischen trägt der erste Weg dagegen gut.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use analysis::{Store, wechsel};
use audio_core::track::{CHANNELS, Track};

/// Fensterlänge für den Pegel.
///
/// Ein Zehntel reicht, um eine Lücke von einer Achtelpause zu unterscheiden,
/// und ist grob genug, dass ein einzelner Nulldurchgang nicht als Stille gilt.
const FENSTER_SEK: f64 = 0.1;

/// Wie leise es sein muss, gemessen am **lauten Teil dieser Aufnahme**.
///
/// Keine feste Zahl. Eine leise aufgenommene Platte wäre sonst durchgehend
/// „still", eine übersteuerte nie — und die Schwelle hätte mehr über den
/// Aufnahmepegel gesagt als über die Musik. Bezugspunkt ist das obere Zehntel
/// der Pegel; wer darunter um mehr als diesen Faktor liegt, ist still.
const STILL_ANTEIL: f32 = 0.02;

/// Wie lange still, damit es als Lücke zählt.
///
/// Eine halbe Sekunde hat auch mancher Break. Ab einer Sekunde ist es keine
/// Musik mehr, sondern ein Ende.
const LUECKE_SEK: f64 = 1.0;

/// Kürzestes Stück, das als Lied durchgeht.
///
/// Kürzeres ist Applaus, ein Jingle oder ein Fehlschnitt. Es wird nicht
/// weggeworfen, sondern gemeldet — wer eine Aufnahme voller kurzer Stücke hat,
/// soll das erfahren und nicht eine leere Ausbeute vorfinden.
const MIND_LIED_SEK: f64 = 40.0;

struct Optionen {
    aufnahme: PathBuf,
    ordner: PathBuf,
    /// Nur nachsehen, nichts schreiben.
    probe: bool,
}

fn main() -> Result<()> {
    let opts = argumente()?;
    let track = Track::decode_file(&opts.aufnahme)
        .with_context(|| format!("{} ließ sich nicht lesen", opts.aufnahme.display()))?;

    println!("Aufnahme: {}", opts.aufnahme.display());
    println!(
        "  {}, {} Hz\n",
        dauer(track.duration_secs()),
        track.sample_rate
    );

    let pegel = pegelkurve(&track);
    let schwelle = stilleschwelle(&pegel);
    let luecken = luecken(&pegel, FENSTER_SEK, schwelle);

    let (schnitte, wie) = if !luecken.is_empty() {
        println!(
            "{} Lücke(n) gefunden — geschnitten wird daran.",
            luecken.len()
        );
        (
            luecken.iter().map(|(a, b)| (a + b) / 2.0).collect(),
            "Lücke",
        )
    } else {
        println!("Keine Lücke gefunden. Bleibt der Klangwechsel — eine Schätzung.");
        let gefunden = wechsel::finden(&track);
        if gefunden.is_empty() {
            println!("\nAuch kein Klangwechsel. Das ist ein Befund, keine Panne:");
            println!("  Entweder steht hier ein einziges Stück, oder die Übergänge sind");
            println!(
                "  länger als der Rückblick von {:.0} s und damit unsichtbar.",
                wechsel::ABSTAND_SEK
            );
            println!("  Geschnitten wird nichts — ein geratener Schnitt wäre schlechter");
            println!("  als keiner.");
            return Ok(());
        }
        println!("{} Klangwechsel gefunden.", gefunden.len());
        (
            gefunden.iter().map(|u| u.beginn).collect::<Vec<f64>>(),
            "Klangwechsel",
        )
    };

    let stuecke: Vec<(f64, f64)> = stuecke(&schnitte, track.duration_secs())
        .into_iter()
        .map(|(a, b)| beschneiden(&pegel, a, b, schwelle, FENSTER_SEK))
        .collect();
    let lang: Vec<&(f64, f64)> = stuecke
        .iter()
        .filter(|(a, b)| b - a >= MIND_LIED_SEK)
        .collect();
    let kurz = stuecke.len() - lang.len();

    println!(
        "\n{} Stück(e), davon {} lang genug für ein Lied.",
        stuecke.len(),
        lang.len()
    );
    if kurz > 0 {
        println!(
            "  {kurz} unter {:.0} s übergangen — Applaus, Jingle oder Fehlschnitt.",
            MIND_LIED_SEK
        );
    }
    if lang.is_empty() {
        return Ok(());
    }

    if opts.probe {
        println!("\n── Was geschnitten würde ({wie}) ────────────");
        for (i, (a, b)) in lang.iter().enumerate() {
            println!(
                "  {:2}  {} bis {}  ({})",
                i + 1,
                dauer(*a),
                dauer(*b),
                dauer(b - a)
            );
        }
        println!("\nNichts geschrieben — `--probe` war gesetzt.");
        return Ok(());
    }

    std::fs::create_dir_all(&opts.ordner)
        .with_context(|| format!("{} ließ sich nicht anlegen", opts.ordner.display()))?;
    let store = Store::new(opts.ordner.join(".analyse"));

    println!("\n── Geschnitten ({wie}) ──────────────────────");
    for (i, (a, b)) in lang.iter().enumerate() {
        let ziel = opts.ordner.join(format!("{:02}.wav", i + 1));
        let von = (a * track.sample_rate as f64) as usize * CHANNELS;
        let bis = ((b * track.sample_rate as f64) as usize * CHANNELS).min(track.samples.len());
        schreibe_wav(&ziel, &track.samples[von..bis], track.sample_rate)
            .with_context(|| format!("{} ließ sich nicht schreiben", ziel.display()))?;

        // Gleich mit analysieren: Ein Stück ohne Grid ist für die Anlage nur
        // eine Datei. Das Sidecar liegt daneben, die Sammlung findet es.
        let stueck = Track::decode_file(&ziel)?;
        let analyse = analysis::analyze(&stueck);
        store.save(&analyse).ok();

        println!(
            "  {:2}  {}  {}  {}  {}",
            i + 1,
            ziel.file_name().unwrap_or_default().to_string_lossy(),
            dauer(b - a),
            analyse
                .bpm
                .map(|b| format!("{b:6.2} BPM"))
                .unwrap_or_else(|| "  kein Grid".into()),
            analyse.musical_key.as_deref().unwrap_or("—"),
        );
    }

    println!("\nDie Sidecars liegen in {}.", store.root().display());
    println!("⚠ Geschnitten heißt nicht geprüft. Wo die Grenze wirklich lag, weiß");
    println!("  nur, wer die Aufnahme kennt — einmal hineinhören lohnt sich.");
    Ok(())
}

/// Pegel in Fenstern von [`FENSTER_SEK`], als RMS.
fn pegelkurve(track: &Track) -> Vec<f32> {
    let fenster = (FENSTER_SEK * track.sample_rate as f64) as usize * CHANNELS;
    track
        .samples
        .chunks(fenster.max(1))
        .map(|t| {
            if t.is_empty() {
                return 0.0;
            }
            (t.iter().map(|x| x * x).sum::<f32>() / t.len() as f32).sqrt()
        })
        .collect()
}

/// Ab welchem Pegel es für **diese** Aufnahme still ist.
fn stilleschwelle(pegel: &[f32]) -> f32 {
    let mut sortiert: Vec<f32> = pegel.iter().copied().filter(|w| w.is_finite()).collect();
    if sortiert.is_empty() {
        return 0.0;
    }
    sortiert.sort_by(|a, b| a.total_cmp(b));
    let oben = sortiert[sortiert.len() * 9 / 10];
    oben * STILL_ANTEIL
}

/// Zusammenhängende stille Bereiche von mindestens [`LUECKE_SEK`].
///
/// Am Anfang und am Ende zählt eine Stille **nicht** als Lücke: Dort trennt sie
/// nichts, sie steht nur davor oder dahinter.
fn luecken(pegel: &[f32], sek_je_wert: f64, schwelle: f32) -> Vec<(f64, f64)> {
    let mindestens = (LUECKE_SEK / sek_je_wert).ceil() as usize;
    let mut aus = Vec::new();
    let mut start: Option<usize> = None;

    for (i, &w) in pegel.iter().enumerate() {
        if w <= schwelle {
            start.get_or_insert(i);
        } else if let Some(s) = start.take()
            && i - s >= mindestens
            && s > 0
        {
            aus.push((s as f64 * sek_je_wert, i as f64 * sek_je_wert));
        }
    }
    aus
}

/// Schiebt die Ränder eines Stücks über die Stille hinweg nach innen.
///
/// Der Schnitt liegt in der Mitte der Lücke, also hängt an jedem Stück eine
/// halbe Lücke Stille. Gemessen: Jedes geschnittene Stück war genau eine
/// Sekunde länger als seine Vorlage. Das ist nicht bloß unschön — die
/// Gliederung sucht ein Intro, und eine Sekunde Stille am Anfang ist genau das,
/// wonach sie sucht. Ein Track ohne Intro bekäme hier einen angedichtet.
fn beschneiden(pegel: &[f32], von: f64, bis: f64, schwelle: f32, sek_je_wert: f64) -> (f64, f64) {
    let last = pegel.len().saturating_sub(1);
    let mut a = ((von / sek_je_wert) as usize).min(last);
    let mut b = ((bis / sek_je_wert).ceil() as usize).min(pegel.len());

    while a < b && pegel[a] <= schwelle {
        a += 1;
    }
    while b > a && pegel[b - 1] <= schwelle {
        b -= 1;
    }
    (a as f64 * sek_je_wert, b as f64 * sek_je_wert)
}

/// Aus Schnittstellen werden Stücke: von 0 bis zum ersten Schnitt und so fort.
fn stuecke(schnitte: &[f64], ende: f64) -> Vec<(f64, f64)> {
    let mut grenzen = vec![0.0];
    grenzen.extend(schnitte.iter().copied().filter(|s| *s > 0.0 && *s < ende));
    grenzen.push(ende);
    grenzen.sort_by(|a, b| a.total_cmp(b));
    grenzen.windows(2).map(|w| (w[0], w[1])).collect()
}

fn dauer(sekunden: f64) -> String {
    let ganz = sekunden.max(0.0) as u64;
    format!("{}:{:02}", ganz / 60, ganz % 60)
}

fn schreibe_wav(pfad: &Path, pcm: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = BufWriter::new(File::create(pfad)?);

    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&(CHANNELS as u16).to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&(rate * CHANNELS as u32 * 2).to_le_bytes())?;
    out.write_all(&((CHANNELS * 2) as u16).to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;

    for sample in pcm {
        out.write_all(&((sample.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())?;
    }
    out.flush()
}

fn argumente() -> Result<Optionen> {
    let mut aufnahme = None;
    let mut ordner = None;
    let mut probe = false;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => {
                hilfe();
                std::process::exit(0);
            }
            "--probe" => probe = true,
            _ if aufnahme.is_none() => aufnahme = Some(PathBuf::from(arg)),
            _ if ordner.is_none() => ordner = Some(PathBuf::from(arg)),
            _ => bail!("zu viele Argumente: {arg}"),
        }
    }

    let aufnahme = aufnahme.context("Aufruf: musik-schneiden <aufnahme> <ordner> [--probe]")?;
    let ordner = match ordner {
        Some(o) => o,
        None if probe => PathBuf::new(),
        None => bail!("Aufruf: musik-schneiden <aufnahme> <ordner> [--probe]"),
    };
    Ok(Optionen {
        aufnahme,
        ordner,
        probe,
    })
}

fn hilfe() {
    println!("Aufruf: musik-schneiden <aufnahme> <ordner> [--probe]");
    println!();
    println!("Zerlegt eine lange Aufnahme in einzelne Lieder, schreibt sie in");
    println!("<ordner> und analysiert jedes gleich mit — Tempo, Tonart,");
    println!("Gliederung liegen danach als Sidecar daneben.");
    println!();
    println!("Geschnitten wird an Lücken. Wo keine sind, am Klangwechsel, und");
    println!("das ist eine Schätzung: Ein gemixtes Set hat zwischen zwei Liedern");
    println!("keine Grenze, sondern eine Überlagerung.");
    println!();
    println!("--probe  zeigt die Schnitte, schreibt aber nichts.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eine_stille_mitten_drin_ist_eine_luecke() {
        // Laut, still, laut — ein Schnitt in der Mitte der Stille.
        let mut p = vec![0.5f32; 300];
        for w in p.iter_mut().skip(100).take(20) {
            *w = 0.0;
        }
        let l = luecken(&p, 0.1, 0.01);
        assert_eq!(l.len(), 1);
        assert!((l[0].0 - 10.0).abs() < 0.01, "{:?}", l[0]);
        assert!((l[0].1 - 12.0).abs() < 0.01, "{:?}", l[0]);
    }

    /// **Stille am Anfang trennt nichts.** Fast jede Aufnahme fängt still an;
    /// wer das als Lücke zählt, bekommt ein erstes Stück aus Nichts.
    #[test]
    fn stille_am_anfang_ist_keine_luecke() {
        let mut p = vec![0.5f32; 300];
        for w in p.iter_mut().take(50) {
            *w = 0.0;
        }
        assert!(luecken(&p, 0.1, 0.01).is_empty());
    }

    /// Eine kurze Pause ist Musik, kein Ende.
    #[test]
    fn eine_kurze_pause_ist_keine_luecke() {
        let mut p = vec![0.5f32; 300];
        for w in p.iter_mut().skip(100).take(5) {
            *w = 0.0;
        }
        assert!(luecken(&p, 0.1, 0.01).is_empty());
    }

    /// **Die Schwelle hängt an der Aufnahme, nicht an einer festen Zahl.**
    /// Dieselbe Musik, einmal leise aufgenommen, muss dieselbe Lücke ergeben.
    #[test]
    fn eine_leise_aufnahme_hat_dieselben_luecken_wie_eine_laute() {
        let mut laut = vec![0.5f32; 300];
        for w in laut.iter_mut().skip(100).take(20) {
            *w = 0.0;
        }
        let leise: Vec<f32> = laut.iter().map(|w| w * 0.02).collect();

        let a = luecken(&laut, 0.1, stilleschwelle(&laut));
        let b = luecken(&leise, 0.1, stilleschwelle(&leise));
        assert_eq!(a.len(), 1);
        assert_eq!(a.len(), b.len());
        assert!((a[0].0 - b[0].0).abs() < 1e-6);
    }

    /// **Die halbe Lücke bleibt nicht am Lied kleben.**
    ///
    /// Gemessen an drei aneinandergehängten Stücken mit zwei Sekunden Pause:
    /// Ohne das Beschneiden war jedes Ergebnis genau eine Sekunde länger als
    /// seine Vorlage — vorn und hinten die halbe Lücke.
    #[test]
    fn die_raender_werden_von_der_stille_befreit() {
        let mut p = vec![0.5f32; 300];
        for w in p.iter_mut().take(10) {
            *w = 0.0;
        }
        for w in p.iter_mut().skip(290) {
            *w = 0.0;
        }
        let (a, b) = beschneiden(&p, 0.0, 30.0, 0.01, 0.1);
        assert!((a - 1.0).abs() < 0.01, "vorn: {a}");
        assert!((b - 29.0).abs() < 0.01, "hinten: {b}");
    }

    /// Ein durchweg stilles Stück wird nicht zu einem negativen.
    #[test]
    fn ein_stilles_stueck_schrumpft_auf_nichts_und_nicht_darunter() {
        let p = vec![0.0f32; 100];
        let (a, b) = beschneiden(&p, 0.0, 10.0, 0.01, 0.1);
        assert!(b >= a, "{a} bis {b}");
    }

    #[test]
    fn aus_schnitten_werden_stuecke() {
        let s = stuecke(&[60.0, 180.0], 300.0);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0], (0.0, 60.0));
        assert_eq!(s[1], (60.0, 180.0));
        assert_eq!(s[2], (180.0, 300.0));
    }

    /// Ohne Schnitt bleibt genau ein Stück — und nicht null.
    #[test]
    fn ohne_schnitt_bleibt_ein_stueck() {
        assert_eq!(stuecke(&[], 300.0), vec![(0.0, 300.0)]);
    }

    /// Ein Schnitt hinter dem Ende darf kein leeres Stück erzeugen.
    #[test]
    fn ein_schnitt_ausserhalb_wird_verworfen() {
        assert_eq!(stuecke(&[400.0], 300.0), vec![(0.0, 300.0)]);
    }
}
