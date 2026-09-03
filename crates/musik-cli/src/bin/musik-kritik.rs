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
//! deshalb nicht auf den Beat genau zurückverfolgen, und der Nullpunkt eines
//! nachträglich geschätzten Rasters ist nicht der Anfang einer Phrase.
//!
//! **Deshalb liest er die Mitschrift mit.** Sie liegt neben dem Mitschnitt und
//! hält fest, welcher Befehl bei welchem Frame ankam und wo die Decks dabei
//! standen. Damit muss er den Griff an den Fader nicht mehr erraten: Der
//! Mitschnitt sagt, was herauskam, die Mitschrift, was gemeint war. Und der
//! Abstand zwischen beiden ist selbst ein Befund — er misst, wie weit die
//! Schätzung danebenliegt, wenn keine Mitschrift da ist.
//!
//! Die Mitschrift ersetzt das Hören ausdrücklich nicht. Sie sagt, wann der
//! Fader bewegt wurde, nicht, ob es gut klang. Wo beide sich widersprechen,
//! ist das ein Befund und keine Fehlerquelle: Ein Griff ohne hörbaren Wechsel
//! heißt, dass die Bewegung nichts bewirkt hat.
//!
//! Was er ausdrücklich **nicht** tut: eine Note geben. Ob ein Set gut war,
//! entscheidet weiterhin jemand, der dabei war. Der Kritiker nimmt ihm die
//! Handwerksfehler ab, nicht das Urteil.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use analysis::wechsel::{self, FENSTER_SEK, Uebergang};
use analysis::{onset, tempo};
use audio_core::track::Track;
use control::mitschrift::{self, Protokoll, Richtung, Stand};

/// Wie weit ein gemessener Übergang von einem festgehaltenen Griff entfernt
/// sein darf, um noch als derselbe zu gelten.
///
/// Eine Blende dauert selten über eine Minute; was weiter auseinanderliegt,
/// gehört nicht zusammen. Lieber „kein Griff in der Nähe" sagen als zwei Dinge
/// zusammenzwingen, die nichts miteinander zu tun haben.
const ZUORDNUNG_SEK: f64 = 45.0;

struct Optionen {
    datei: PathBuf,
    mitschrift: Option<PathBuf>,
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

    let absicht = match &opts.mitschrift {
        Some(pfad) => match mitschrift::lesen(pfad) {
            Ok(p) => {
                absicht_bericht(pfad, &p);
                Some(Absicht {
                    rate: p.kopf.rate,
                    einsaetze: einsaetze(&p),
                    griffe: griffe(&p),
                })
            }
            Err(e) => {
                // Kein Abbruch: Der Mitschnitt bleibt lesbar, auch wenn die
                // Mitschrift es nicht ist. Verschwiegen wird es nicht.
                println!("Mitschrift {}: {e}\n", pfad.display());
                None
            }
        },
        None => {
            println!("Keine Mitschrift daneben — Beginn und Phrasenlage bleiben geschätzt.");
            println!("  Sie entsteht beim Aufnehmen von selbst und heißt wie der");
            println!(
                "  Mitschnitt, nur mit der Endung .{}.\n",
                mitschrift::ENDUNG
            );
            None
        }
    };

    let kurve = wechsel::wechselkurve(&track);
    let uebergaenge = wechsel::uebergaenge_finden(&kurve, FENSTER_SEK);

    pegel_bericht(&track);

    if uebergaenge.is_empty() {
        println!("\nKein Übergang gefunden.");
        println!("  Entweder lief nur ein Track, oder der Wechsel war zu leise für");
        println!("  die Schwelle. Das ist ein Befund, keine Entwarnung.");
        // Ein Griff ohne hörbaren Wechsel ist selbst ein Befund: Die Bewegung
        // hat nichts bewirkt. Das darf hier nicht untergehen.
        if let Some(a) = absicht.as_ref().filter(|a| !a.griffe.is_empty()) {
            println!(
                "\n⚠ Die Mitschrift kennt {} Griff(e), gehört hat man keinen.",
                a.griffe.len()
            );
            println!("   Entweder stand der Kanal zu, oder die Bewegung war zu klein.");
        }
        return Ok(());
    }

    println!("\n{} Übergänge gefunden:", uebergaenge.len());
    for (i, u) in uebergaenge.iter().enumerate() {
        println!("\n── Übergang {} ──────────────────────────────", i + 1);
        uebergang_bericht(&track, u, absicht.as_ref());
    }

    if let Some(a) = &absicht {
        schaetzfehler_bericht(&uebergaenge, a);
    }

    Ok(())
}

/// Was die Anlage vorhatte, gelesen aus der Mitschrift.
struct Absicht {
    rate: u32,
    griffe: Vec<Griff>,
    /// Wann welches Deck losgelaufen ist — `(deck, frame)`, nullbasiert.
    einsaetze: Vec<(usize, u64)>,
}

impl Absicht {
    fn sekunden(&self, frame: u64) -> f64 {
        frame as f64 / self.rate.max(1) as f64
    }

    /// Der Griff, der einem gemessenen Beginn am nächsten liegt.
    ///
    /// Bewusst über den Abstand und nicht über die Reihenfolge: Nicht jeder
    /// Griff wird hörbar, und nicht jeder hörbare Wechsel hat einen Griff —
    /// eine Paarung nach Position wäre eine Behauptung.
    fn naechster(&self, sekunde: f64) -> Option<(&Griff, f64)> {
        self.griffe
            .iter()
            .map(|g| (g, self.sekunden(g.frame) - sekunde))
            .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .filter(|(_, ab)| ab.abs() <= ZUORDNUNG_SEK)
    }
}

/// Eine Reglerbewegung, wie die Mitschrift sie festgehalten hat.
///
/// **Das ist die Absicht, nicht das Ergebnis.** Ob der Griff hörbar war, sagt
/// weiterhin nur der Mitschnitt — hier steht, wann er geschah und wo die Decks
/// dabei standen.
struct Griff {
    frame: u64,
    control: String,
    ziel: f64,
    /// Bestellte Länge in Beats.
    beats: f64,
    /// Nummer im Plan, über die sich das Ende wiederfindet.
    plan: Option<u64>,
    staende: Vec<Stand>,
    ende: Option<Ende>,
}

/// Wie eine Bewegung ausging.
struct Ende {
    frame: u64,
    /// `fertig`, `abgeloest` oder `abgebrochen` — die Worte des Zeitplans.
    ausgang: String,
    staende: Vec<Stand>,
}

/// Liest die Bewegungen aus einer Mitschrift.
fn griffe(p: &Protokoll) -> Vec<Griff> {
    let mut gefunden = Vec::new();

    for (i, e) in p.ereignisse.iter().enumerate() {
        if e.richtung != Richtung::Befehl {
            continue;
        }
        let Some(rest) = e.text.strip_prefix("ramp ") else {
            continue;
        };
        let mut worte = rest.split_whitespace();
        let (Some(control), Some(ziel), Some(beats)) = (worte.next(), worte.next(), worte.next())
        else {
            continue;
        };
        let (Ok(ziel), Ok(beats)) = (ziel.parse::<f64>(), beats.parse::<f64>()) else {
            continue;
        };

        // Genau die nächste Zeile, nicht die nächste passende: Die Antwort
        // folgt dem Befehl unmittelbar. Weiter zu suchen hieße, bei einem
        // abgelehnten Befehl die Nummer des übernächsten zu erwischen.
        let plan = p
            .ereignisse
            .get(i + 1)
            .filter(|n| n.richtung == Richtung::Meldung)
            .and_then(|n| plan_nummer(&n.text));

        let ende = plan.and_then(|id| ende_von(p, i + 1, id));

        gefunden.push(Griff {
            frame: e.frame,
            control: control.to_string(),
            ziel,
            beats,
            plan,
            staende: e.staende.clone(),
            ende,
        });
    }

    gefunden
}

/// Die Nummer aus einer Zeile, die von einem Plan spricht.
fn plan_nummer(text: &str) -> Option<u64> {
    let mut worte = text.split_whitespace();
    while let Some(w) = worte.next() {
        if w == "plan" {
            return worte.next()?.parse().ok();
        }
    }
    None
}

/// Sucht die Meldung, mit der ein Auftrag zu Ende ging.
fn ende_von(p: &Protokoll, ab: usize, id: u64) -> Option<Ende> {
    const AUSGAENGE: [&str; 3] = ["fertig", "abgeloest", "abgebrochen"];

    p.ereignisse.iter().skip(ab).find_map(|e| {
        if e.richtung != Richtung::Meldung || plan_nummer(&e.text) != Some(id) {
            return None;
        }
        let wort = e.text.split_whitespace().nth(2)?;
        AUSGAENGE.contains(&wort).then(|| Ende {
            frame: e.frame,
            ausgang: wort.to_string(),
            staende: e.staende.clone(),
        })
    })
}

/// Was die Mitschrift für sich genommen sagt.
fn absicht_bericht(pfad: &Path, p: &Protokoll) {
    println!("Mitschrift: {}", pfad.display());
    println!(
        "  {} Ereignisse, {} Bewegungen",
        p.ereignisse.len(),
        griffe(p).len()
    );
    if p.unlesbar > 0 {
        println!(
            "  ⚠ {} Zeilen ließen sich nicht lesen — die Mitschrift hat Lücken.",
            p.unlesbar
        );
    }
    println!();
}

/// Wann welches Deck eingesetzt hat.
///
/// Ein Deck, das genau bei einem Griff losläuft, steht dabei noch auf seinem
/// Einstiegspunkt — die Position wird erst nach dem nächsten Audioblock
/// veröffentlicht. Genau darin liegt aber der Befund: Wo im *eigenen* Raster
/// der eingehende Track anfängt, ist die Frage, die ein Cue-Punkt beantwortet.
fn einsaetze(p: &Protokoll) -> Vec<(usize, u64)> {
    p.ereignisse
        .iter()
        .filter(|e| e.richtung == Richtung::Befehl)
        .filter_map(|e| {
            let rest = e.text.strip_prefix("set deck")?;
            let (nummer, rest) = rest.split_once(".play")?;
            let an = matches!(rest.trim(), "1" | "true" | "on");
            let deck: usize = nummer.parse().ok()?;
            (an && deck > 0).then_some((deck - 1, e.frame))
        })
        .collect()
}

/// Ein Griff mit allem, was die Mitschrift über ihn weiß.
fn griff_bericht(g: &Griff, rate: u32, einzug: &str, einsaetze: &[(usize, u64)]) {
    let sek = |frame: u64| frame as f64 / rate.max(1) as f64;
    println!(
        "{einzug}Griff bei {:.2} s: {} → {:.2} über {:.0} Beats",
        sek(g.frame),
        g.control,
        g.ziel,
        g.beats
    );

    for s in &g.staende {
        let einsatz = einsaetze.contains(&(s.deck, g.frame));
        // Ein stehendes Deck bekommt keine Phrasenlage. Sie wäre ausrechenbar
        // und würde gelesen wie „es kam neben der Eins herein" — dabei kam es
        // überhaupt nicht herein.
        let Some(lage) = s.phrasenlage() else {
            println!("{einzug}  deck{}: steht bei Beat {:.2}", s.deck + 1, s.beat);
            continue;
        };
        let hinweis = if lage < 0.25 {
            " — auf der Eins"
        } else if (s.phrase_beats - lage) < 0.25 {
            " — knapp vor der Eins"
        } else if einsatz {
            " ⚠ nicht auf seiner Eins"
        } else {
            ""
        };
        let was = if einsatz {
            "setzt hier ein bei Beat"
        } else {
            "Beat"
        };
        println!(
            "{einzug}  deck{}: {was} {:.2}, {lage:.2} in die Phrase ({:.0}){hinweis}",
            s.deck + 1,
            s.beat,
            s.phrase_beats
        );
    }
    if g.staende.is_empty() {
        println!("{einzug}  Kein Deck hatte ein Beatgrid — die Phrasenlage fehlt.");
    }

    match &g.ende {
        Some(e) => {
            let dauer = sek(e.frame) - sek(g.frame);
            print!("{einzug}  {} nach {dauer:.1} s", e.ausgang);
            // Was wirklich an Beats vergangen ist, weiß nur die Mitschrift:
            // Wer am Tempo dreht, verschiebt Sekunden, nicht Beats.
            match (g.staende.first(), e.staende.first()) {
                (Some(a), Some(b)) if a.deck == b.deck => {
                    let gefahren = b.beat - a.beat;
                    print!(" · {gefahren:.1} von {:.0} Beats", g.beats);
                    if (gefahren - g.beats).abs() > 1.0 {
                        print!(" ⚠");
                    }
                }
                _ => {}
            }
            println!();
        }
        None => println!(
            "{einzug}  Kein Ende in der Mitschrift{}.",
            match g.plan {
                Some(id) => format!(" (Plan {id} läuft noch oder das Set brach ab)"),
                None => " — der Befehl kam nicht durch".to_string(),
            }
        ),
    }
}

/// Wie weit die Schätzung aus dem Klang danebenlag.
///
/// **Das ist die Zahl, die diese ganze Übung rechtfertigt.** Ohne Mitschrift
/// bleibt sie unbekannt, und dann hält man die Schätzung für die Wahrheit.
fn schaetzfehler_bericht(uebergaenge: &[Uebergang], a: &Absicht) {
    let abweichungen: Vec<f64> = uebergaenge
        .iter()
        .filter_map(|u| a.naechster(u.beginn).map(|(_, ab)| ab))
        .collect();
    if abweichungen.is_empty() {
        return;
    }

    let mittel = abweichungen.iter().map(|d| d.abs()).sum::<f64>() / abweichungen.len() as f64;
    let groesste = abweichungen
        .iter()
        .copied()
        .max_by(|x, y| x.abs().total_cmp(&y.abs()))
        .unwrap_or(0.0);

    println!("\n── Was die Schätzung taugt ──────────────────");
    println!(
        "  {} von {} Übergängen ließen sich einem Griff zuordnen.",
        abweichungen.len(),
        uebergaenge.len()
    );
    println!("  Im Mittel lag die Schätzung {mittel:.1} s daneben, am weitesten {groesste:+.1} s.");
    println!("  Ein negatives Vorzeichen heißt: zu spät geschätzt — genau der Fehler,");
    println!("  den eine lange Blende erzwingt, weil ihr Anfang unhörbar ist.");
}

/// Wie breit ein Pegelwert ist, in Sekunden.
///
/// Steht hier als Konstante, weil zwei Stellen sie brauchen: das Rechnen und
/// das Suchen nach einer Sekunde im Ergebnis. **Sie standen auseinander** —
/// gerechnet wurde ein Wert je Sekunde, gesucht wurde mit zwei Sekunden je
/// Wert —, und damit beschrieb der Pegelbericht einer Blende eine Stelle bei
/// halber Zeit. Am laufenden Programm fiel es auf: Der Kritiker meldete
/// „64 % Einbruch in der Mitte" eines gefahrenen Übergangs und beschrieb
/// damit den Break des ausgehenden Tracks, fünfzehn Sekunden früher, an dem
/// kein Regler bewegt worden war. Der Befund darüber — „das ist die
/// Crossfader-Kurve" — war dann Rat zu einer Stelle, die niemand gefahren hat.
const PEGEL_SEK: f64 = 1.0;

/// Pegel über den ganzen Mitschnitt, ein Wert je [`PEGEL_SEK`].
fn pegel(track: &Track) -> Vec<f32> {
    // Mal `CHANNELS`, weil `samples` verschränkt liegt. Zahlenmäßig dasselbe
    // wie das frühere `* 2`; ausgeschrieben, damit ein drittes Deck oder ein
    // Mono-Mitschnitt es nicht stillschweigend verschiebt.
    let fenster = (track.sample_rate as f64 * PEGEL_SEK) as usize * audio_core::track::CHANNELS;
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

/// Wie ein Pegelloch in einer Blende zu erklären ist.
///
/// Zwei Ursachen sehen im Mitschnitt gleich aus und wollen Verschiedenes: eine
/// Kurve, die in der Mitte Leistung verliert — und zwei Tracks, die schlicht
/// verschieden laut sind. Trennen lassen sie sich an den Rändern: Steht der
/// Pegel *nach* dem Übergang deutlich anders als davor, war es das Material,
/// und eine härtere Kurve verdeckt den Unterschied nur.
///
/// Bis dahin sagte der Kritiker in beiden Fällen „das ist die Crossfader-Kurve"
/// — an einem gefahrenen Übergang gemessen war das falsch: Ein Drop mit Kick
/// und Bass ging in ein reines Akkord-Intro über, zwölf Dezibel leiser, und
/// keine Kurve der Welt ändert daran etwas.
#[derive(Debug, PartialEq, Eq)]
enum Loch {
    /// Kein nennenswerter Einbruch.
    Keins,
    /// Die Ränder sind verschieden laut — eine Stufe, keine Delle.
    Stufe,
    /// Die Ränder sind gleich laut, die Mitte nicht.
    Kurve,
    /// Auf einer Seite gibt es nichts zu vergleichen.
    ///
    /// Ein Übergang in den ersten Sekunden eines Mitschnitts hat kein Davor,
    /// und einer am Ende kein Danach. Dann ist der Einbruch echt, seine
    /// Ursache aber von hier aus nicht zu bestimmen — und das zu sagen ist
    /// besser, als die häufigere der beiden zu raten.
    Unklar,
}

/// Ab wie viel Prozent ein Einbruch überhaupt genannt wird.
const EINBRUCH_PROZENT: f32 = 15.0;

/// Ab welchem Verhältnis der Ränder es eine Stufe heißt.
///
/// 0,7 sind gut drei Dezibel — hörbar verschieden, und mehr als der Abstand
/// zwischen zwei Abschnitten desselben Stücks. Gewählt und nicht gemessen;
/// gemessen ist nur der Fall, der dazu führte (0,20 gegen 0,38, also 0,53),
/// und der liegt weit auf der einen Seite.
const STUFE_ANTEIL: f32 = 0.7;

/// Wie viele Pegelwerte links und rechts als „außen" gelten.
const RAND_WERTE: usize = 4;

/// Die Pegelwerte um einen Übergang: das Minimum darin, das Lauteste davor,
/// das Lauteste danach.
///
/// Eigene Funktion, weil hier eine Sekunde in einen Index umgerechnet wird und
/// genau diese Umrechnung schon einmal um den Faktor zwei danebenlag. In einer
/// `println!`-Strecke sieht man das nie.
fn pegel_um(p: &[f32], beginn: f64, ende: f64) -> Option<(f32, f32, f32)> {
    let idx = |s: f64| (s / PEGEL_SEK) as usize;
    let (a, b) = (idx(beginn), idx(ende).min(p.len()));
    if b <= a || b > p.len() {
        return None;
    }
    let innen = p[a..b].iter().copied().fold(f32::MAX, f32::min);
    let davor = p[a.saturating_sub(RAND_WERTE)..a]
        .iter()
        .copied()
        .fold(0.0f32, f32::max);
    let danach = p[b..(b + RAND_WERTE).min(p.len())]
        .iter()
        .copied()
        .fold(0.0f32, f32::max);
    Some((innen, davor, danach))
}

fn loch(innen: f32, davor: f32, danach: f32) -> (f32, Loch) {
    let aussen = davor.max(danach);
    if aussen <= 0.001 {
        return (0.0, Loch::Keins);
    }
    let verlust = 100.0 * (1.0 - innen / aussen);
    if verlust <= EINBRUCH_PROZENT {
        return (verlust, Loch::Keins);
    }
    if davor <= 0.001 || danach <= 0.001 {
        return (verlust, Loch::Unklar);
    }
    // Symmetrisch: Ob der eingehende Track leiser ist oder der ausgehende war,
    // ändert nichts daran, dass die Kurve nicht die Ursache ist.
    let leiser = davor.min(danach);
    if leiser < aussen * STUFE_ANTEIL {
        (verlust, Loch::Stufe)
    } else {
        (verlust, Loch::Kurve)
    }
}

fn uebergang_bericht(track: &Track, u: &Uebergang, absicht: Option<&Absicht>) {
    let dauer = u.ende - u.beginn;
    println!(
        "  Beginn {:>7.2} s (±{:.0} s), Dauer {:.1} s, Stärke {:.2}",
        u.beginn, u.unschaerfe, dauer, u.hoehe
    );

    // --- Was gemeint war ------------------------------------------------
    // Steht vor der Schätzung, weil es sie ablöst: Wo die Mitschrift den
    // Griff kennt, ist alles Zurückverfolgen aus dem Klang nur noch die
    // Gegenprobe.
    let zugeordnet = absicht.and_then(|a| a.naechster(u.beginn));
    if let (Some(a), Some((g, ab))) = (absicht, &zugeordnet) {
        println!("  ── laut Mitschrift ──");
        griff_bericht(g, a.rate, "  ", &a.einsaetze);
        println!(
            "    Die Schätzung aus dem Klang liegt {:.1} s {}.",
            ab.abs(),
            if *ab < 0.0 { "zu spät" } else { "zu früh" }
        );
    } else if absicht.is_some() {
        println!("  Kein Griff in der Mitschrift in der Nähe — der Wechsel kam nicht");
        println!("  von einem Regler, oder er stand nicht im Plan.");
    }

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

            // Kein Urteil aus dem Klang allein — und mit Mitschrift ein
            // anderes, kleineres. Zwei Gründe, beide hart:
            //
            // Das Fenster ist eine Sekunde breit, bei 126 BPM also zwei Beats.
            // Damit lässt sich nicht feststellen, ob ein Einsatz auf dem
            // Schlag sitzt — die Auflösung ist gröber als die Frage.
            //
            // Und der Anker des Detektors ist der erste starke Schlag im
            // Analysefenster, nicht der Anfang einer Phrase. „Sechs Beats
            // neben der Eins" wäre gegen einen willkürlichen Nullpunkt
            // gemessen.
            //
            // Die Mitschrift schließt den zweiten Grund halb: Sie kennt den
            // Beat, den die *Anlage* meinte, und damit lässt sich prüfen, ob
            // die Anlage getan hat, was sie vorhatte. Ob ihr Beat 0 auch
            // musikalisch ein Downbeat ist, weiß erst die Strukturanalyse.
            if zugeordnet.is_some() {
                println!("  · Die Phrasenlage oben ist gegen die Rechnung der Anlage gemessen:");
                println!("     Sie sagt, ob die Anlage traf, was sie vorhatte. Ob ihr Beat 0");
                println!("     auch musikalisch die Eins ist, weiß erst die Strukturanalyse.");
            } else {
                println!("  · Beat- und Phrasenlage sind so nicht beurteilbar: Die Unschärfe");
                println!("     (±{unschaerfe_beats:.0} Beats) ist größer als der Versatz, und der");
                println!("     Nullpunkt des Rasters ist nicht der Anfang einer Phrase.");
            }
        }
        None => println!("  Raster davor nicht erkennbar — Phrasenlage nicht beurteilbar."),
    }

    // --- Pegelloch ------------------------------------------------------
    let p = pegel(track);
    if let Some((innen, davor, danach)) = pegel_um(&p, u.beginn, u.ende) {
        let aussen = davor.max(danach);
        if aussen > 0.001 {
            let (verlust, art) = loch(innen, davor, danach);
            println!("  Pegel in der Blende: {innen:.3} gegen {aussen:.3} außen");
            match art {
                Loch::Keins => {
                    println!("  ✓ Pegel hält über die Blende ({verlust:.0} % Abweichung).")
                }
                Loch::Kurve => {
                    println!("  ⚠ {verlust:.0} % Einbruch in der Mitte, und beide Seiten sind");
                    println!("     gleich laut ({davor:.3} / {danach:.3}). Das ist die Kurve;");
                    println!("     `master.crossfader_curve` härter stellen.");
                }
                Loch::Unklar => {
                    println!("  ⚠ {verlust:.0} % Einbruch — aber auf einer Seite fehlt der");
                    println!("     Vergleich (davor {davor:.3}, danach {danach:.3}). Ob das die");
                    println!("     Kurve war oder zwei verschieden laute Tracks, sagt dieser");
                    println!("     Mitschnitt nicht.");
                }
                Loch::Stufe => {
                    println!("  ⚠ {verlust:.0} % Einbruch — aber die Ränder sind verschieden");
                    println!("     laut: davor {davor:.3}, danach {danach:.3}. Dann liegt es am");
                    println!("     Material und nicht an der Kurve — Gain, oder eine andere");
                    println!("     Stelle zum Einsteigen.");
                }
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
    let mut mitschrift = None;
    let mut warte_auf_mitschrift = false;

    for arg in std::env::args().skip(1) {
        if warte_auf_mitschrift {
            mitschrift = Some(PathBuf::from(arg));
            warte_auf_mitschrift = false;
            continue;
        }
        match arg.as_str() {
            "-h" | "--help" => {
                hilfe();
                std::process::exit(0);
            }
            "--mitschrift" => warte_auf_mitschrift = true,
            "--ohne-mitschrift" => mitschrift = Some(PathBuf::new()),
            _ => datei = Some(PathBuf::from(arg)),
        }
    }
    if warte_auf_mitschrift {
        bail!("--mitschrift braucht eine Datei");
    }

    let Some(datei) = datei else {
        hilfe();
        bail!("keine Datei angegeben");
    };
    if !Path::new(&datei).exists() {
        bail!("{} gibt es nicht", datei.display());
    }

    // Ohne Angabe die daneben: Die Anlage legt sie beim Aufnehmen von selbst
    // dorthin, und wer sie erst nennen müsste, ließe sie meistens liegen.
    // `--ohne-mitschrift` trägt einen leeren Pfad und heißt: ausdrücklich
    // nicht — dafür gibt es einen Grund, siehe `hilfe`.
    let mitschrift = match mitschrift {
        Some(p) if p.as_os_str().is_empty() => None,
        Some(p) => {
            if !p.exists() {
                bail!("{} gibt es nicht", p.display());
            }
            Some(p)
        }
        None => {
            let daneben = datei.with_extension(mitschrift::ENDUNG);
            daneben.exists().then_some(daneben)
        }
    };

    Ok(Optionen { datei, mitschrift })
}

fn hilfe() {
    println!("Aufruf: musik-kritik <mitschnitt.wav> [--mitschrift <datei>]");
    println!();
    println!("Liest einen Mitschnitt und benennt, was messbar ist: wo ein Übergang");
    println!("liegt, wie lang er dauert, was der Pegel dabei macht und ob das Tempo");
    println!("durchhält.");
    println!();
    println!("Liegt eine Mitschrift daneben (gleicher Name, Endung .mitschrift),");
    println!("liest er sie mit. Dann muss er den Griff an den Fader nicht mehr aus");
    println!("dem Klang erraten, und die Phrasenlage wird messbar.");
    println!();
    println!("--ohne-mitschrift ignoriert sie. Das ist kein Sparmodus, sondern die");
    println!("Probe aufs Exempel: Nur so sieht man, wie weit die Schätzung");
    println!("danebenliegt, wenn nur der Klang da ist.");
    println!();
    println!("Er gibt keine Note. Ob ein Set gut war, entscheidet, wer dabei war.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protokoll(zeilen: &[&str]) -> Protokoll {
        let mut text = String::from("# musik-mitschrift 1\n# mitschnitt set.wav\n# rate 48000\n");
        text.push_str(&zeilen.join("\n"));
        mitschrift::aus_text(&text).expect("Mitschrift lesbar")
    }

    /// Ein Wert je Sekunde — die Zahl, an der der Pegelbericht hängt.
    ///
    /// Stand hier einmal ein Fenster von `rate * 2` **Samples**, war ein Wert
    /// eine halbe Sekunde breit, während `uebergang_bericht` mit zwei
    /// Sekunden je Wert suchte. Der Bericht beschrieb dann eine Stelle bei
    /// halber Zeit, ohne dass irgendetwas nach einem Fehler aussah.
    #[test]
    fn ein_pegelwert_ist_eine_sekunde_breit() {
        let rate = 48_000u32;
        let sekunden = 10;
        let track = Track {
            samples: vec![0.5f32; rate as usize * audio_core::track::CHANNELS * sekunden],
            sample_rate: rate,
            stems: Vec::new(),
        };
        assert_eq!(track.duration_secs(), sekunden as f64);
        assert_eq!(pegel(&track).len(), sekunden);
        assert_eq!(PEGEL_SEK, 1.0);
    }

    /// Eine Sekunde ist ein Wert — auch beim Suchen.
    ///
    /// Der Fehler, den diese Prüfung fängt, war unsichtbar: Der Bericht sah
    /// vollständig aus und nannte plausible Zahlen, nur stammten sie aus der
    /// halben Zeit. Deshalb ein Verlauf, in dem jede Sekunde anders klingt —
    /// dann sagt die Zahl, wo geschaut wurde.
    #[test]
    fn die_pegelwerte_kommen_aus_der_richtigen_sekunde() {
        let p: Vec<f32> = (0..40).map(|i| i as f32 / 100.0).collect();
        let (innen, davor, danach) = pegel_um(&p, 20.0, 30.0).expect("Fenster liegt im Verlauf");
        assert!((innen - 0.20).abs() < 1e-6, "innen {innen}");
        assert!((davor - 0.19).abs() < 1e-6, "davor {davor}");
        assert!((danach - 0.33).abs() < 1e-6, "danach {danach}");

        // Am Anfang gibt es kein Davor, und das ist kein Fehler.
        let (_, davor, _) = pegel_um(&p, 0.0, 10.0).expect("Fenster liegt im Verlauf");
        assert_eq!(davor, 0.0);

        // Über das Ende hinaus wird nichts erfunden.
        assert_eq!(pegel_um(&p, 39.0, 39.0), None);
        assert_eq!(pegel_um(&p, 50.0, 60.0), None);
    }

    /// Der Befund, der am gefahrenen Übergang entstand.
    #[test]
    fn ein_leiserer_track_ist_keine_kurve() {
        // Drop mit Kick und Bass hinein, reines Akkord-Intro heraus.
        let (verlust, art) = loch(0.081, 0.381, 0.200);
        assert_eq!(art, Loch::Stufe);
        assert!(verlust > 70.0, "{verlust}");

        // Gleich laute Ränder, Delle in der Mitte: dann ist es die Kurve.
        assert_eq!(loch(0.20, 0.40, 0.38).1, Loch::Kurve);

        // Und andersherum ist es genauso wenig die Kurve.
        assert_eq!(loch(0.081, 0.200, 0.381).1, Loch::Stufe);
    }

    /// Ohne Rand kein Urteil: lieber „weiß nicht" als die häufigere Ursache.
    #[test]
    fn ein_fehlender_rand_ist_keine_stufe() {
        // Ein Übergang gleich zu Beginn: kein Davor.
        assert_eq!(loch(0.081, 0.0, 0.381).1, Loch::Unklar);
        // Und einer am Ende: kein Danach.
        assert_eq!(loch(0.081, 0.381, 0.0).1, Loch::Unklar);
    }

    /// Ein Pegel, der hält, ist kein Befund — auch nicht mit Rauschen drauf.
    #[test]
    fn ohne_einbruch_wird_nichts_gemeldet() {
        assert_eq!(loch(0.36, 0.40, 0.39).1, Loch::Keins);
        // Stille auf beiden Seiten: nichts zu vergleichen, nichts zu melden.
        assert_eq!(loch(0.0, 0.0, 0.0), (0.0, Loch::Keins));
    }

    #[test]
    fn die_plannummer_steht_in_beiden_richtungen() {
        assert_eq!(
            plan_nummer("ok plan 3 ramp master.crossfader nach 1"),
            Some(3)
        );
        assert_eq!(
            plan_nummer("plan 12 fertig master.crossfader 1.0000"),
            Some(12)
        );
        assert_eq!(plan_nummer("err kein Deck mit Beatgrid"), None);
        assert_eq!(plan_nummer("ok plan viele"), None);
    }

    /// Der Regelfall: bestellt, angenommen, zu Ende gefahren.
    #[test]
    fn aus_befehl_und_meldung_wird_ein_ganzer_griff() {
        let p = protokoll(&[
            "480000 10.000 deck1=64.000/16 > ramp master.crossfader 1 32",
            "480100 10.002 deck1=64.001/16 < ok plan 3 ramp master.crossfader nach 1 über 32 Beats",
            "1200000 25.000 deck1=96.000/16 < plan 3 fertig master.crossfader 1.0000",
        ]);
        let g = griffe(&p);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].frame, 480_000);
        assert_eq!(g[0].control, "master.crossfader");
        assert_eq!(g[0].beats, 32.0);
        assert_eq!(g[0].plan, Some(3));
        assert_eq!(g[0].staende[0].beat, 64.0);
        assert_eq!(
            g[0].staende[0].phrasenlage(),
            Some(0.0),
            "64 ist eine Phrasengrenze"
        );

        let ende = g[0].ende.as_ref().expect("das Ende fehlt");
        assert_eq!(ende.ausgang, "fertig");
        assert_eq!(ende.staende[0].beat - g[0].staende[0].beat, 32.0);
    }

    /// Ein abgelehnter Befehl darf sich nicht die Nummer des nächsten borgen.
    ///
    /// Ohne das hinge ein `plan 4 fertig` am falschen Griff, und der Bericht
    /// behauptete eine Blende, die nie lief.
    #[test]
    fn ein_abgelehnter_befehl_bekommt_keine_fremde_nummer() {
        let p = protokoll(&[
            "0 0.000 > ramp master.crossfader 1 32",
            "0 0.000 < err kein Deck mit Beatgrid als Taktgeber",
            "480000 10.000 deck1=64.000/16 > ramp channel1.eq_low 0 8",
            "480100 10.002 deck1=64.001/16 < ok plan 4 ramp channel1.eq_low nach 0 über 8 Beats",
            "600000 12.500 deck1=68.000/16 < plan 4 fertig channel1.eq_low 0.0000",
        ]);
        let g = griffe(&p);
        assert_eq!(g.len(), 2);
        assert_eq!(
            g[0].plan, None,
            "der abgelehnte Befehl hat eine Nummer bekommen"
        );
        assert!(
            g[0].ende.is_none(),
            "der abgelehnte Befehl hat ein fremdes Ende"
        );
        assert_eq!(g[1].plan, Some(4));
        assert_eq!(
            g[1].ende.as_ref().map(|e| e.ausgang.as_str()),
            Some("fertig")
        );
    }

    /// Eine abgelöste Rampe ist kein Fehler im Lesen, sondern ein Befund:
    /// Jemand anders hat den Regler angefasst.
    #[test]
    fn eine_abgeloeste_rampe_wird_als_solche_gelesen() {
        let p = protokoll(&[
            "0 0.000 deck1=0.000/16 > ramp channel1.fader 0 16",
            "0 0.000 deck1=0.000/16 < ok plan 1 ramp channel1.fader nach 0 über 16 Beats",
            "96000 2.000 deck1=4.000/16 < plan 1 abgeloest channel1.fader — jemand anders hat den Regler",
        ]);
        let g = griffe(&p);
        assert_eq!(
            g[0].ende.as_ref().map(|e| e.ausgang.as_str()),
            Some("abgeloest")
        );
    }

    /// Zugeordnet wird über den Abstand, nicht über die Reihenfolge — und was
    /// zu weit weg ist, wird gar nicht zugeordnet.
    #[test]
    fn der_griff_wird_ueber_den_abstand_gesucht() {
        let a = Absicht {
            rate: 48_000,
            griffe: griffe(&protokoll(&[
                "480000 10.000 deck1=64.000/16 > ramp master.crossfader 1 32",
                "480100 10.002 deck1=64.001/16 < ok plan 1 ramp master.crossfader nach 1",
                "9600000 200.000 deck1=512.000/16 > ramp master.crossfader 0 32",
                "9600100 200.002 deck1=512.001/16 < ok plan 2 ramp master.crossfader nach 0",
            ])),
            einsaetze: Vec::new(),
        };
        assert_eq!(a.griffe.len(), 2);

        // Vier Sekunden zu spät geschätzt: negativer Abstand.
        let (g, ab) = a.naechster(14.0).expect("kein Griff gefunden");
        assert_eq!(g.frame, 480_000);
        assert!((ab + 4.0).abs() < 1e-6, "{ab}");

        // Näher am zweiten.
        assert_eq!(a.naechster(195.0).expect("kein Griff").0.frame, 9_600_000);

        // Und dazwischen gehört nichts zusammen.
        assert!(
            a.naechster(120.0).is_none(),
            "zwei Dinge wurden zusammengezwungen, die 100 s auseinanderliegen"
        );
    }

    /// Der Einsatz eines Decks ist ein eigenes Ereignis — und die Frage, ob er
    /// auf der Eins des *eingehenden* Tracks lag, ist die, die ein Cue-Punkt
    /// beantwortet.
    #[test]
    fn ein_einsatz_wird_als_solcher_erkannt() {
        let p = protokoll(&[
            "1116160 23.253 deck1=48.012/16 deck2=-0.998/16~ > set deck2.play 1",
            "1116160 23.253 deck1=48.012/16 deck2=-0.998/16 < ok deck2.play 1",
            "1116160 23.253 deck1=48.012/16 deck2=-0.998/16 > ramp master.crossfader 1 32",
        ]);
        assert_eq!(einsaetze(&p), vec![(1, 1_116_160)]);
    }

    /// Ein Stopp ist kein Einsatz, und eine Frage erst recht nicht.
    #[test]
    fn ein_stopp_ist_kein_einsatz() {
        let p = protokoll(&[
            "0 0.000 deck1=0.000/16 > set deck1.play 0",
            "0 0.000 deck1=0.000/16 < ok deck1.play 0",
            "48000 1.000 deck1=2.000/16 < ok deck2.play 1",
        ]);
        assert!(einsaetze(&p).is_empty(), "{:?}", einsaetze(&p));
    }
}
