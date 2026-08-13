//! Tonarterkennung.
//!
//! Nach dem Tempo die zweite Hälfte der Trackauswahl: Zwei Stücke im gleichen
//! Takt können trotzdem gegeneinander klingen, wenn ihre Tonarten nicht
//! zusammenpassen. Wer harmonisch mischt, braucht sie.
//!
//! **Warum selbst gebaut.** Die beiden verbreiteten Verfahren — libKeyFinder
//! und der QM Key Detector — stehen unter GPL, und das schlösse den Weg zu
//! VibeMind (MIT, zur Veröffentlichung vorgesehen) wieder zu. Siehe
//! `docs/BAUSTEINE.md`. Also der klassische Weg von Hand:
//!
//! 1. **Chroma.** Das Spektrum wird auf zwölf Halbtonklassen gefaltet — alle
//!    Oktaven eines Tons landen im selben Fach. Was übrig bleibt, ist die
//!    Tonverteilung ohne Lage und ohne Klangfarbe.
//! 2. **Vergleich.** Diese Verteilung wird gegen vierundzwanzig Profile
//!    korreliert: zwölf Dur- und zwölf Moll-Tonarten. Die Profile stammen von
//!    Krumhansl und Kessler (1982) — Messwerte aus Hörversuchen, wie gut ein
//!    Ton in eine Tonart passt.
//! 3. **Konfidenz** aus der Güte der besten Anpassung. Ein Stück ohne klare
//!    Tonart soll das sagen, statt eine zu erfinden.
//!
//! Der Wert selbst — Name, Camelot-Notation, harmonische Nachbarschaft —
//! liegt in [`audio_core::Tonart`]. Hier steht nur, wie man ihn aus Audio
//! gewinnt.

use rustfft::{num_complex::Complex, FftPlanner};

use audio_core::track::CHANNELS;
pub use audio_core::Tonart;

/// Fenster für die Chroma-Analyse.
///
/// Deutlich länger als beim Onset (1024): Dort geht es um zeitliche Schärfe,
/// hier um Frequenzauflösung. Wie weit sie reicht, steht bei [`MIN_HZ`] — sie
/// ist der Grund für die untere Grenze.
pub const FENSTER: usize = 8192;
pub const SPRUNG: usize = 4096;

/// Unterhalb dieser Anpassungsgüte sieht die Tonverteilung nach gar keiner
/// Tonart aus.
///
/// **Gemessen wird die Güte, nicht der Vorsprung.** Der naheliegende Weg wäre
/// der Abstand zum zweitbesten Treffer — der taugt aber nicht: Perkussion kam
/// damit auf 0,062 und eine echte Moll-Folge auf 0,088, also praktisch
/// dasselbe.
///
/// **Der Wert ist an echten Aufnahmen geeicht, nicht an synthetischen.** Das
/// ist der Unterschied, an dem die erste Fassung danebenlag: Selbstgebaute
/// Akkordfolgen erreichen 0,92 bis 0,95, echte Aufnahmen kommen über 0,82 kaum
/// hinaus. Eine an synthetischem Material geeichte Schwelle von 0,80 wies
/// vier von fünf echten Tracks ab — dieselbe Falle wie beim Tempo, wo eine an
/// Klick-Tracks geeichte Schwelle echte Musik verwarf.
///
/// Geeicht an fünf Aufnahmen, gegengeprüft über die Stabilität in
/// 30-Sekunden-Abschnitten. Eine echte Tonart bleibt über den Track stehen,
/// eine erfundene springt:
///
/// | r    | Abschnitte einig | Urteil |
/// |------|------------------|--------|
/// | 0,81 | 4 von 4          | Tonart |
/// | 0,79 | 5 von 7          | Tonart |
/// | 0,77 | 6 von 7          | Tonart |
/// | 0,71 | 4 von 6, springt | keine  |
/// | 0,56 | 4 von 8, springt | keine  |
///
/// **Warum echte Musik niedriger liegt, ist nicht, wofür man es hält.** Nicht
/// Rauschen und nicht Perkussion: Gleichmäßiges Rauschen hebt alle zwölf
/// Klassen um denselben Betrag an, und eine Korrelation ist gegen so einen
/// Offset unempfindlich — gemessen sinkt die Güte selbst bei grobem Rauschen
/// und lauten Drums nur von 0,95 auf 0,90. Was sie drückt, ist **fremde
/// Harmonik**: Töne außerhalb der Tonart, Zwischenteile in anderen Tonarten,
/// Stimmen mit gleitender Tonhöhe. Eine Aufnahme bei 0,75 ist also keine
/// schlechte Aufnahme, sondern eine harmonisch bewegte.
///
/// **Diese Schwelle lässt sich nicht durch einen Test bewachen.** Um sie zu
/// prüfen, bräuchte es synthetisches Material im Bereich echter Musik, und
/// genau das ist nicht herstellbar: Selbst eine Tritonus-Modulation über ein
/// Drittel der Länge kommt nur auf 0,864, weil Akkorde aus sauberen
/// Obertonreihen über einem starren Halbtonraster immer zu gut korrelieren.
/// Was hier bewacht wird, ist die Tabelle oben — wer den Wert ändert, muss
/// gegen echte Aufnahmen nachmessen.
///
/// **Fünf Aufnahmen sind eine dünne Grundlage.** Der Abstand zwischen 0,71 und
/// 0,77 ist schmal, und mehr Material würde ihn schärfen. In dieser Richtung
/// zu irren ist aber die richtige: Ein Track ohne Angabe fehlt in der
/// harmonischen Suche, ein Track mit falscher Angabe führt beim Auflegen in
/// den Griff daneben.
const MIN_PASSUNG: f32 = 0.72;

/// Wie stark die Terz mindestens gegenüber dem Grundton stehen muss.
///
/// **Die Terz entscheidet über Dur und Moll, also muss sie klingen.** Ein
/// Sägezahn-Bass bringt über seinen fünften Teilton eine große Terz mit, ohne
/// dass sie jemand gespielt hätte; die Erkennung liest daraus verlässlich Dur.
/// Bei einer Bassfigur auf A-A-D-C kommt so „A" statt „Am" heraus — der
/// Grundton stimmt, aber auf dem Camelot-Rad ist das 11B statt 8A und damit
/// der Unterschied zwischen passt und passt nicht.
///
/// Der Teilton verrät sich durch seine Schwäche. Gemessen als Verhältnis
/// Terz zu Grundton:
///
/// | Material                          | Deutung | Terz |
/// |-----------------------------------|---------|------|
/// | Akkordfolge in Dur                | richtig | 0,64 |
/// | Akkordfolge in Moll               | richtig | 0,95 |
/// | Demo-Deck A (Drums, Bass, Fläche) | richtig | 0,59 |
/// | Demo-Deck B (Drums, Bass, Fläche) | richtig | 0,90 |
/// | Bassfigur A-A-D-C, keine Akkorde  | **falsch** | 0,36 |
/// | Bassfigur E-E-A-G, keine Akkorde  | **falsch** | 0,44 |
///
/// Wer nur Bass und Drums hat, bekommt deshalb **gar keine** Tonart. Das
/// kostet Treffer und vermeidet Fehlgriffe; in diese Richtung zu irren ist
/// die richtige.
///
/// Beide Schwellen sind an synthetischem Material gemessen und gegen eine
/// echte Sammlung nie geprüft — derselbe Vorbehalt wie beim Tempo.
///
/// Der Vorsprung vor der zweitbesten Deutung bleibt bewusst ungeprüft: Er ist
/// gerade zwischen einer Tonart und ihrer Paralleltonart klein, und deren
/// Verwechslung schadet beim Mischen nicht — auf dem Camelot-Rad tragen beide
/// dieselbe Zahl.
const MIN_TERZ: f32 = 0.5;

/// Tiefster und höchster berücksichtigter Ton.
///
/// **Die untere Grenze ist keine Geschmacksfrage, sondern Arithmetik.** Ein
/// Bin ist bei 8192 Punkten und 44,1 kHz 5,4 Hz breit; ein Halbton ist bei
/// C2 (65 Hz) nur 3,9 Hz breit. Unterhalb von rund 200 Hz fallen also mehrere
/// Halbtöne in denselben Bin, und die Zuordnung wird zur Münze — ausgerechnet
/// dort, wo Kick und Bass die meiste Energie haben. Erst ab etwa 200 Hz
/// kommen zwei Bins auf einen Halbton.
///
/// Der Bass geht dadurch nicht verloren: Seine Obertöne liegen im Band, und
/// [`HARMONISCH`] rechnet sie auf den Grundton zurück.
const MIN_HZ: f32 = 200.0;
/// Darüber sitzen vor allem Obertöne und Becken.
const MAX_HZ: f32 = 2_000.0;

/// Wie viele Obertöne auf ihren Grundton zurückgerechnet werden.
///
/// Ohne das liest ein einzelner Bass-Ton wie ein Akkord: Eine Sägezahnwelle
/// auf A hat Teiltöne auf A, E, C#, G — also fast einen A-Dur-Septakkord, und
/// die Erkennung nimmt ihn für bare Münze. Deshalb bekommt jeder Bin nicht nur
/// seine eigene Halbtonklasse, sondern auch die der möglichen Grundtöne
/// darunter, mit fallendem Gewicht.
///
/// Sechs Teiltöne, weil der siebte 0,31 Halbtöne neben dem Raster liegt und
/// damit mehr Unsinn stiftet als Nutzen (das Verfahren geht auf Gómez 2006
/// zurück, die HPCP mit „harmonic contribution").
const HARMONISCH: usize = 6;
const ABFALL: f32 = 0.6;

/// Krumhansl-Kessler-Profile: wie gut jeder Halbton in eine Dur- bzw.
/// Molltonart passt, gemessen in Hörversuchen (1982).
const DUR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MOLL: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

#[derive(Debug, Clone, Copy)]
pub struct Ergebnis {
    pub tonart: Tonart,
    /// Wie gut die Tonverteilung auf das beste Profil passt, 0..1.
    pub konfidenz: f32,
}

/// Ermittelt die Tonart aus interleaved Stereo.
///
/// `None`, wenn das Material zu kurz ist oder keine klare Tonart zeigt —
/// Perkussion, Rauschen, eine Aufnahme ohne Harmonik, ein Bass ohne Akkorde.
/// Eine geratene Tonart wäre beim harmonischen Mischen schlimmer als keine.
pub fn erkenne(samples: &[f32], sample_rate: u32) -> Option<Ergebnis> {
    let chroma = chroma(samples, sample_rate)?;
    let (tonart, konfidenz) = beste_tonart(&chroma);

    // Zwei Fragen, zwei Schwellen: Sieht das überhaupt nach einer Tonart aus,
    // und klingt die Terz, die über Dur und Moll entscheidet?
    let taugt = konfidenz >= MIN_PASSUNG && terz_anteil(&chroma, tonart) >= MIN_TERZ;
    taugt.then_some(Ergebnis { tonart, konfidenz })
}

/// Wie stark die Terz gegenüber dem Grundton steht — siehe [`MIN_TERZ`].
fn terz_anteil(chroma: &[f32; 12], tonart: Tonart) -> f32 {
    let grund = chroma[tonart.grundton as usize % 12];
    let terz = chroma[(tonart.grundton as usize + if tonart.dur { 4 } else { 3 }) % 12];
    if grund > 0.0 {
        terz / grund
    } else {
        0.0
    }
}

/// Die zwölf Halbtonklassen, aufsummiert über den ganzen Track.
pub fn chroma(samples: &[f32], sample_rate: u32) -> Option<[f32; 12]> {
    let mono: Vec<f32> = samples
        .chunks_exact(CHANNELS)
        .map(|f| f.iter().sum::<f32>() / CHANNELS as f32)
        .collect();
    if mono.len() < FENSTER {
        return None;
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FENSTER);
    let fenster = hann(FENSTER);

    // Welche Halbtonklassen bekommt welcher Bin, und mit welchem Gewicht?
    // Einmal vorab, statt in jeder Runde Logarithmen je Bin zu rechnen.
    let bin_klasse = bin_zuordnung(sample_rate);
    let oberton = oberton_gewichte();

    let mut chroma = [0.0f32; 12];
    let mut puffer: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); FENSTER];
    let mut start = 0;

    while start + FENSTER <= mono.len() {
        for (i, platz) in puffer.iter_mut().enumerate() {
            *platz = Complex::new(mono[start + i] * fenster[i], 0.0);
        }
        fft.process(&mut puffer);

        for (bin, klasse) in bin_klasse.iter().enumerate() {
            let Some(k) = klasse else { continue };
            let staerke = puffer[bin].norm();
            // Der Bin könnte der erste, zweite, dritte … Teilton sein. Jede
            // Möglichkeit bekommt eine Stimme, die weiter unten leiser wird.
            for (versatz, gewicht) in &oberton {
                let ziel = (*k as i32 - versatz).rem_euclid(12) as usize;
                chroma[ziel] += staerke * gewicht;
            }
        }
        start += SPRUNG;
    }

    let summe: f32 = chroma.iter().sum();
    if summe <= 0.0 {
        return None;
    }
    for wert in &mut chroma {
        *wert /= summe;
    }
    Some(chroma)
}

/// Für jeden Teilton: um wie viele Halbtöne der Grundton darunter liegt, und
/// wie stark er gewichtet wird.
///
/// Der `n`-te Teilton liegt `12·log2(n)` Halbtöne über seinem Grundton. Das
/// ist nur für Zweierpotenzen glatt; gerundet wird auf das Halbtonraster, und
/// der größte Fehler bei sechs Teiltönen sind 0,14 Halbtöne (beim fünften).
fn oberton_gewichte() -> Vec<(i32, f32)> {
    (1..=HARMONISCH)
        .map(|n| {
            let halbtoene = (12.0 * (n as f32).log2()).round() as i32;
            (halbtoene, ABFALL.powi(n as i32 - 1))
        })
        .collect()
}

/// Ordnet jedem FFT-Bin eine Halbtonklasse zu — oder keine.
fn bin_zuordnung(sample_rate: u32) -> Vec<Option<u8>> {
    (0..FENSTER / 2)
        .map(|bin| {
            let hz = bin as f32 * sample_rate as f32 / FENSTER as f32;
            if !(MIN_HZ..=MAX_HZ).contains(&hz) {
                return None;
            }
            // Halbtöne über A4 = 440 Hz gezählt, dann auf C bezogen.
            let halbtoene = 12.0 * (hz / 440.0).log2();
            let klasse = (halbtoene.round() as i32 + 9).rem_euclid(12);
            Some(klasse as u8)
        })
        .collect()
}

/// Korreliert die Verteilung mit allen vierundzwanzig Profilen.
///
/// Zurück kommt die beste Deutung und wie gut sie passt — nicht, wie weit sie
/// vor der zweitbesten liegt. Warum, steht bei [`MIN_PASSUNG`].
fn beste_tonart(chroma: &[f32; 12]) -> (Tonart, f32) {
    let mut beste = (Tonart::neu(0, true), f32::MIN);

    for grundton in 0..12u8 {
        for dur in [true, false] {
            let profil = if dur { &DUR } else { &MOLL };
            // Das Profil wird gedreht, statt das Chroma zu drehen — dasselbe
            // Ergebnis, aber der Grundton bleibt lesbar.
            let gedreht: Vec<f32> = (0..12)
                .map(|i| profil[(i + 12 - grundton as usize) % 12])
                .collect();

            let r = korrelation(chroma, &gedreht);
            if r > beste.1 {
                beste = (Tonart { grundton, dur }, r);
            }
        }
    }

    (beste.0, beste.1.clamp(0.0, 1.0))
}

fn korrelation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let mittel_a = a.iter().sum::<f32>() / n;
    let mittel_b = b.iter().sum::<f32>() / n;

    let mut oben = 0.0;
    let mut links = 0.0;
    let mut rechts = 0.0;
    for (x, y) in a.iter().zip(b) {
        let dx = x - mittel_a;
        let dy = y - mittel_b;
        oben += dx * dy;
        links += dx * dx;
        rechts += dy * dy;
    }

    let unten = (links * rechts).sqrt();
    if unten > 0.0 {
        oben / unten
    } else {
        0.0
    }
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let x = std::f32::consts::TAU * i as f32 / n as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::akkordfolge;

    const RATE: u32 = 44_100;

    #[test]
    fn eine_dur_folge_wird_als_dur_erkannt() {
        let ergebnis = erkenne(&akkordfolge(0, false), RATE).expect("keine Tonart erkannt");
        assert_eq!(ergebnis.tonart.name(), "C", "erkannt: {:?}", ergebnis);
        assert!(ergebnis.tonart.dur);
    }

    #[test]
    fn eine_moll_folge_wird_als_moll_erkannt() {
        let ergebnis = erkenne(&akkordfolge(9, true), RATE).expect("keine Tonart erkannt");
        assert_eq!(ergebnis.tonart.name(), "Am", "erkannt: {:?}", ergebnis);
        assert!(!ergebnis.tonart.dur);
    }

    /// Der eigentliche Beweis, dass die Kette stimmt.
    ///
    /// Dieselbe Musik um drei Halbtöne verschoben muss dieselbe Verschiebung
    /// im Ergebnis zeigen. Das prüft die ganze Zuordnung von Frequenz zu
    /// Halbtonklasse, ohne dass irgendwo eine Wahrheit hinterlegt sein müsste.
    #[test]
    fn eine_verschobene_folge_verschiebt_die_tonart_mit() {
        for versatz in [1, 3, 5, 7] {
            let ergebnis =
                erkenne(&akkordfolge(versatz, false), RATE).expect("keine Tonart erkannt");
            let erwartet = (versatz as u8) % 12;
            assert_eq!(
                ergebnis.tonart.grundton,
                erwartet,
                "um {versatz} verschoben ergibt {}",
                ergebnis.tonart.name()
            );
        }
    }

    /// Bass und Drums allein bekommen keine Tonart.
    ///
    /// Das ist der Fall, an dem die erste Fassung sichtbar gescheitert ist: Eine
    /// Bassfigur auf A, A, D, C — a-Moll bzw. dessen Paralleltonart C-Dur —
    /// wurde als **cis-Moll** gelesen, und das mit hoher Konfidenz.
    ///
    /// Der Grundton stimmt inzwischen, das Tongeschlecht nicht: Der Sägezahn
    /// bringt seine eigene große Terz mit, und daraus wird verlässlich Dur.
    /// „A" statt „Am" ist auf dem Camelot-Rad 11B statt 8A und damit ein
    /// Fehlgriff. Abgefangen wird das von [`MIN_TERZ`], nicht von der
    /// Anpassungsgüte — die liegt hier bei 0,85 und damit im Bereich echter
    /// Stücke.
    #[test]
    fn bass_und_drums_allein_bekommen_keine_tonart() {
        // Halbtöne über C2: A, A, D, C.
        let samples = crate::testing::bass_mit_kick(&[9, 9, 2, 0], 8);
        let ergebnis = erkenne(&samples, RATE);
        assert!(
            ergebnis.is_none(),
            "Tonart aus einer Bassfigur behauptet: {:?}",
            ergebnis.map(|e| (e.tonart.name(), e.konfidenz))
        );

        // Und zwar an der Terz, nicht an der Güte — sonst würde der Test auch
        // grün bleiben, wenn die Schwelle irgendwann alles Tonale mit
        // aussperrt.
        let chroma = chroma(&samples, RATE).expect("Chroma");
        let (tonart, guete) = beste_tonart(&chroma);
        assert!(
            guete >= MIN_PASSUNG,
            "die Güte allein hätte schon abgelehnt: {guete:.3}"
        );
        assert!(
            terz_anteil(&chroma, tonart) < MIN_TERZ,
            "die Terz steht bei {:.2} und hätte durchgelassen",
            terz_anteil(&chroma, tonart)
        );
    }

    /// Der Grundton stimmt — auch wenn er allein nicht ausgereicht hat.
    ///
    /// Ohne diesen Test wäre nicht belegt, dass die Erkennung an solchem
    /// Material überhaupt etwas Richtiges sieht und nur zu Recht schweigt.
    /// Geprüft wird deshalb unterhalb der Schwelle, direkt auf der Deutung.
    #[test]
    fn eine_bassfigur_trifft_wenigstens_ihren_grundton() {
        for (noten, grundton, name) in [(&[9, 9, 2, 0], 9u8, "A"), (&[16, 16, 9, 7], 4u8, "E")] {
            let chroma = chroma(&crate::testing::bass_mit_kick(noten, 8), RATE).expect("Chroma");
            let (tonart, _) = beste_tonart(&chroma);
            assert_eq!(
                tonart.grundton,
                grundton,
                "Bass auf {name} wird als {} gedeutet",
                tonart.name()
            );
        }
    }

    /// Was die Obertonrückrechnung bringt, in Zahlen.
    ///
    /// Ein Sägezahn-Bass legt auf jede gespielte Note gleich eine halbe
    /// Obertonreihe. Ohne [`HARMONISCH`] stehen deshalb Töne, die nie gespielt
    /// wurden, über solchen, die es wurden: Bei A-A-D-C rutschte das gespielte
    /// D hinter ein G#, das im Stück nicht vorkommt. Zurückgerechnet stehen
    /// alle drei gespielten Töne wieder über dem Mittel der übrigen.
    #[test]
    fn gespielte_toene_stehen_ueber_den_obertoenen() {
        // Halbtöne über C2: A, A, D, C.
        let gespielt = [9usize, 2, 0];
        let chroma =
            chroma(&crate::testing::bass_mit_kick(&[9, 9, 2, 0], 8), RATE).expect("Chroma");

        let uebrig: f32 = (0..12)
            .filter(|i| !gespielt.contains(i))
            .map(|i| chroma[i])
            .sum::<f32>()
            / (12 - gespielt.len()) as f32;

        for ton in gespielt {
            assert!(
                chroma[ton] > uebrig,
                "{} steht mit {:.3} unter dem Mittel der ungespielten Töne ({uebrig:.3}); Chroma {chroma:?}",
                audio_core::tonart::TOENE[ton],
                chroma[ton]
            );
        }
    }

    /// Der Grund für [`MIN_PASSUNG`], als Test festgehalten.
    ///
    /// Der Vorsprung vor der zweitbesten Deutung trennt tonales von
    /// untonalem Material **nicht**: Perkussion lag damit über einer echten
    /// Moll-Folge. Die Anpassungsgüte trennt sauber, und dieser Test hielte
    /// eine Rückkehr zum alten Maß auf.
    #[test]
    fn die_passung_trennt_tonales_von_perkussion() {
        let tonal = chroma(&akkordfolge(0, false), RATE).expect("kein Chroma");
        let (_, gut) = beste_tonart(&tonal);

        let perkussiv = chroma(&crate::testing::click_track(128.0, RATE, 20.0, 0.0), RATE)
            .expect("kein Chroma");
        let (_, schlecht) = beste_tonart(&perkussiv);

        assert!(gut >= MIN_PASSUNG, "Akkordfolge nur {gut:.3}");
        assert!(schlecht < MIN_PASSUNG, "Perkussion schon {schlecht:.3}");
        assert!(
            gut - schlecht > 0.2,
            "die beiden liegen zu dicht: {gut:.3} gegen {schlecht:.3}"
        );
    }

    #[test]
    fn rauschen_bekommt_keine_tonart_angedichtet() {
        let mut seed = 0x1234_5678u32;
        let samples: Vec<f32> = (0..RATE as usize * 8)
            .flat_map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let v = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
                [v * 0.3, v * 0.3]
            })
            .collect();

        let ergebnis = erkenne(&samples, RATE);
        assert!(
            ergebnis.is_none(),
            "Rauschen wurde als {:?} gedeutet",
            ergebnis.map(|e| e.tonart.name())
        );
    }

    #[test]
    fn zu_kurzes_material_ergibt_none() {
        assert!(erkenne(&[0.1; 200], RATE).is_none());
        assert!(erkenne(&[], RATE).is_none());
    }
}
