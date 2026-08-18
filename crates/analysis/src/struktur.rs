//! Gliederung eines Tracks: Intro, Aufbau, Drop, Break, Outro.
//!
//! **Die größte Lücke in der Analyse.** Die Anlage kennt Tempo, Tonart und die
//! Wellenform. Sie kennt nicht die Stellen, an denen ein Übergang überhaupt
//! sitzen darf — und deshalb ist „blende aus, während das Outro läuft" bisher
//! nicht ausdrückbar gewesen, weil das Wort Outro nicht existierte.
//!
//! Die Mitschrift (S1b) hat das beim ersten gemessenen Übergang sofort
//! vorgeführt: Der ausgehende Track lag sauber auf seiner Phrasengrenze, der
//! eingehende setzte auf Sekunde 0 ein — fünfzehn Beats neben seiner eigenen
//! Eins. Ohne Gliederung gibt es keinen besseren Einstiegspunkt als „von
//! vorn".
//!
//! # Wie
//!
//! Gerechnet wird **auf dem vorhandenen Beatgrid**, nicht auf der Uhr. Der
//! Track zerfällt in Phrasen von [`PHRASE_BEATS`] Beats, und je Phrase stehen
//! drei Zahlen:
//!
//! | Größe | Was sie unterscheidet |
//! | --- | --- |
//! | Pegel | laut gegen leise — Drop gegen Intro |
//! | Bass | **die entscheidende**: ein Break ist Bass weg, ein Drop ist Bass da |
//! | Dichte | Schläge gegen keine Schläge — Intro ohne Drums gegen Break mit |
//!
//! Ein Break und ein Intro sind beide leise; sie unterscheiden sich darin, ob
//! Perkussion läuft. Ein Drop und ein Aufbau sind beide laut; sie unterscheiden
//! sich im Bass. Zwei Zahlen reichen dafür nicht, vier wären Zierde.
//!
//! Höhen (Hi-Hats, Brillanz) fehlen bewusst. Sie würden trennen, was die
//! anderen drei schon trennen, und jede Größe, die nichts entscheidet, macht
//! die Schwellen nur schwerer zu begründen.
//!
//! Eine Grenze liegt dort, wo sich das Klangbild über eine Phrasengrenze hinweg
//! ändert: Der Abstand zwischen dem Mittel der [`KERN`] Phrasen davor und dem
//! der [`KERN`] danach ist die Neuheit, und ihre lokalen Spitzen sind die
//! Grenzen. Dass sie auf Phrasengrenzen liegen, ist keine nachträgliche
//! Rundung, sondern folgt aus der Bauart — in dieser Musik wechseln die Teile
//! dort und nirgends sonst.
//!
//! # Was daran belegt ist und was nicht
//!
//! **Die Grenzen sind geprüft**, an eigens gebautem Material mit bekannter
//! Gliederung: Intro, Aufbau, Drop, Break, Drop, Outro. Sie werden gefunden.
//!
//! **Die Benennung ist es nicht.** Sie hängt an Quantilen des Tracks selbst —
//! keine absolute Zahl, weil eine solche Schwelle in diesem Projekt schon
//! viermal an echten Aufnahmen zerbrochen ist, nachdem sie an synthetischem
//! Material gut aussah. Relativ zu messen macht das besser, aber nicht wahr:
//! Ob „lauteste 25 % mit viel Bass" auf echten Produktionen wirklich der Drop
//! ist, weiß erst, wer es an echten Produktionen mit bekannter Gliederung
//! nachmisst. Bis dahin stehen die Zahlen neben jedem Namen im Bericht, damit
//! man ihm widersprechen kann.

use audio_core::track::CHANNELS;

pub use audio_core::struktur::{Abschnitt, Art, Struktur, PHRASE_BEATS};

use crate::onset::OnsetEnvelope;
use crate::tempo::Beatgrid;

/// Obere Grenze des Bassbands, in Hertz.
///
/// Kick und Bassline liegen darunter, alles Melodische darüber. Gefiltert wird
/// mit zwei Einpolern hintereinander — 12 dB je Oktave, also weich. Für die
/// Frage „ist der Bass da oder weg" reicht das; wer eine Trennung mit Flanke
/// braucht, baut sie, wenn eine Messung sie verlangt.
pub const BASS_HZ: f32 = 160.0;

/// Wie viele Phrasen je Seite in die Neuheit eingehen.
///
/// **Eine.** Der erste Entwurf nahm zwei, aus der Sorge, eine einzelne Phrase
/// sei jeder Variation ausgeliefert. Nachgemessen war das Gegenteil der Fall:
/// Ein Abschnitt von zwei Phrasen ist genauso lang wie ein Kern aus zwei je
/// Seite, und dann vergleicht der Kern nie zwei reine Teile, sondern immer
/// Mischungen. Am gebauten Material stand die echte Grenze deshalb *niedriger*
/// als die Schulter daneben.
///
/// Die Glättung, die den Kern rechtfertigen sollte, steckt schon in der Phrase:
/// Ein Merkmal ist der Mittelwert über sechzehn Beats, bei 128 BPM also über
/// siebeneinhalb Sekunden. Ein ausgelassener Kick verschwindet darin von
/// selbst. Mit einer Phrase je Seite trennen sich die Grenzen am gebauten
/// Material um das Fünfundzwanzigfache vom Rest, mit zweien nur noch um das
/// Doppelte.
pub const KERN: usize = 1;

/// Kürzester Abschnitt, in Phrasen.
///
/// Ein Teil von einer halben Phrase ist kein Teil, sondern eine Variation.
pub const MIN_PHRASEN: usize = 2;

/// Wie weit sich das Klangbild über eine Grenze hinweg ändern muss.
///
/// Gemessen als Abstand in einem Raum, in dem jede der drei Größen auf ihren
/// eigenen Höchstwert im Track bezogen ist. 0,15 heißt also: „das Klangbild
/// verschiebt sich um fünfzehn Prozent dessen, was dieser Track selbst an
/// Spannweite hat".
///
/// **Der erste Entwurf nahm Median plus Streuung der Neuheit** und fand
/// deshalb gar nichts: Wenn jede zweite Phrase eine Grenze ist, sind die
/// Ausschläge nicht die Ausnahme, sondern die Hälfte der Verteilung — sie
/// heben den Median so weit an, dass sie sich selbst überdecken. Ein
/// Bezugspunkt aus dem Material selbst ist hier der falsche, weil das Material
/// bimodal ist.
///
/// **An eigenem Material geeicht, nicht an echten Aufnahmen.** Dort liegen die
/// Grenzen um das Fünfundzwanzigfache über dem Rest, die genaue Zahl ist also
/// beliebig; bei echten Produktionen wird der Abstand kleiner sein, und dann
/// entscheidet sie. Genau der Satz, der in diesem Projekt schon viermal die
/// Vorstufe eines Fehlers war — er steht hier, damit die Zahl nicht für
/// geprüft gehalten wird.
pub const SCHWELLE: f32 = 0.15;

/// Gliedert einen Track.
///
/// `None`, wenn es kein Grid gibt oder der Track kürzer ist als ein paar
/// Phrasen — aus zwei Phrasen lässt sich keine Gliederung lesen, und eine
/// geratene wäre schlechter als keine.
pub fn analysiere(
    samples: &[f32],
    sample_rate: u32,
    grid: Beatgrid,
    env: &OnsetEnvelope,
) -> Option<Struktur> {
    let fenster = phrasenfenster(samples.len() / CHANNELS, sample_rate, grid)?;
    if fenster.len() < 2 * KERN + MIN_PHRASEN {
        return None;
    }

    let merkmale = merkmale(samples, sample_rate, env, &fenster);
    let z = normieren(&merkmale);
    let neuheit = neuheit(&z);
    let grenzen = grenzen(&neuheit, fenster.len());

    let mut abschnitte = zusammenfassen(&fenster, &merkmale, &grenzen, sample_rate, grid);
    benennen(&mut abschnitte);

    Some(Struktur {
        abschnitte,
        phrase_beats: PHRASE_BEATS,
    })
}

/// Anfang jedes Phrasenfensters in Sample-Frames.
///
/// Das Gitter hängt am Anker des Beatgrids, nicht an Frame 0: Der Vorlauf vor
/// der ersten Eins gehört zu keiner Phrase.
fn phrasenfenster(frames: usize, sample_rate: u32, grid: Beatgrid) -> Option<Vec<(u64, u64)>> {
    if grid.bpm <= 0.0 {
        return None;
    }
    let je_beat = 60.0 / grid.bpm as f64 * sample_rate as f64;
    let je_phrase = (je_beat * PHRASE_BEATS) as u64;
    if je_phrase == 0 {
        return None;
    }

    let start = grid.anchor_frames % je_phrase;
    let mut aus = Vec::new();
    let mut von = start;
    while von + je_phrase <= frames as u64 {
        aus.push((von, von + je_phrase));
        von += je_phrase;
    }

    // Der Rest hinter der letzten vollen Phrase, wenn er mindestens eine halbe
    // ist. Ohne das fällt das Outro durch: Ein Track endet selten glatt auf
    // einer Phrasengrenze, und was übrig bleibt, ist genau der Teil, den man
    // sucht. Am gebauten Material blieben so von zwei Outro-Phrasen nur eine
    // übrig — zu wenig für einen eigenen Abschnitt, und der Track hatte
    // plötzlich kein Ende mehr.
    let rest = frames as u64 - von;
    if rest >= je_phrase / 2 {
        aus.push((von, frames as u64));
    }

    (!aus.is_empty()).then_some(aus)
}

/// Pegel, Bass und Dichte je Phrase.
struct Merkmale {
    pegel: Vec<f32>,
    bass: Vec<f32>,
    dichte: Vec<f32>,
}

impl Merkmale {
    fn len(&self) -> usize {
        self.pegel.len()
    }

    /// Die drei Größen einer Phrase als Vektor.
    fn zeile(&self, i: usize) -> [f32; 3] {
        [self.pegel[i], self.bass[i], self.dichte[i]]
    }
}

fn merkmale(
    samples: &[f32],
    sample_rate: u32,
    env: &OnsetEnvelope,
    fenster: &[(u64, u64)],
) -> Merkmale {
    let mono = mono(samples);
    let tief = tiefpass(&mono, sample_rate);

    let mut pegel = Vec::with_capacity(fenster.len());
    let mut bass = Vec::with_capacity(fenster.len());
    let mut dichte = Vec::with_capacity(fenster.len());

    for &(von, bis) in fenster {
        let (a, b) = (von as usize, (bis as usize).min(mono.len()));
        pegel.push(rms(&mono[a..b]));
        bass.push(rms(&tief[a..b]));

        // Die Hüllkurve läuft in ihrer eigenen Rate, nicht in Sample-Frames.
        let je = env.hop.max(1);
        let (ea, eb) = (
            (a / je).min(env.values.len()),
            (b / je).min(env.values.len()),
        );
        dichte.push(if eb > ea {
            env.values[ea..eb].iter().sum::<f32>() / (eb - ea) as f32
        } else {
            0.0
        });
    }

    Merkmale {
        pegel,
        bass,
        dichte,
    }
}

fn mono(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks_exact(CHANNELS)
        .map(|f| f.iter().sum::<f32>() / CHANNELS as f32)
        .collect()
}

/// Zwei Einpoler hintereinander — 12 dB je Oktave.
fn tiefpass(mono: &[f32], sample_rate: u32) -> Vec<f32> {
    let dt = 1.0 / sample_rate as f32;
    let rc = 1.0 / (std::f32::consts::TAU * BASS_HZ);
    let a = dt / (rc + dt);

    let mut aus = Vec::with_capacity(mono.len());
    let (mut z1, mut z2) = (0.0f32, 0.0f32);
    for &x in mono {
        z1 += a * (x - z1);
        z2 += a * (z1 - z2);
        aus.push(z2);
    }
    aus
}

fn rms(werte: &[f32]) -> f32 {
    if werte.is_empty() {
        return 0.0;
    }
    (werte.iter().map(|v| v * v).sum::<f32>() / werte.len() as f32).sqrt()
}

/// Jede Größe auf ihren Höchstwert im Track bezogen.
///
/// Nötig, weil der Pegel sonst den Abstand beherrschte — allein deshalb, weil
/// seine Zahlen größer sind als die der Dichte.
///
/// **Nicht als z-Wert.** Der erste Entwurf teilte durch die Streuung, und damit
/// bekam gleichförmiges Material dieselbe Spannweite wie ein Track voller
/// Wechsel: Rauschen, auf Streuung 1 gestreckt, sieht aus wie Struktur. Ein
/// Track ohne Abschnitte muss aber Werte nahe beieinander behalten, damit die
/// Neuheit nahe null bleibt und er keine bekommt.
fn normieren(m: &Merkmale) -> Vec<[f32; 3]> {
    let spalten = [&m.pegel, &m.bass, &m.dichte];
    let spitzen: [f32; 3] =
        std::array::from_fn(|k| spalten[k].iter().copied().fold(0.0f32, f32::max).max(1e-9));

    (0..m.len())
        .map(|i| {
            let z = m.zeile(i);
            std::array::from_fn(|k| z[k] / spitzen[k])
        })
        .collect()
}

/// Wie fremd sich die Phrasen vor und nach einer Grenze sind.
///
/// Ein Wert je Phrasengrenze: `neuheit[i]` gehört vor Phrase `i`.
fn neuheit(z: &[[f32; 3]]) -> Vec<f32> {
    (0..z.len())
        .map(|i| {
            let vor = mittel(&z[i.saturating_sub(KERN)..i]);
            let nach = mittel(&z[i..(i + KERN).min(z.len())]);
            match (vor, nach) {
                (Some(a), Some(b)) => (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt(),
                _ => 0.0,
            }
        })
        .collect()
}

fn mittel(teil: &[[f32; 3]]) -> Option<[f32; 3]> {
    if teil.is_empty() {
        return None;
    }
    let n = teil.len() as f32;
    Some(std::array::from_fn(|k| {
        teil.iter().map(|z| z[k]).sum::<f32>() / n
    }))
}

/// Die Phrasen, vor denen ein neuer Abschnitt beginnt.
fn grenzen(neuheit: &[f32], phrasen: usize) -> Vec<usize> {
    let mut aus = Vec::new();
    let mut letzte = 0usize;

    // Einschließlich der oberen Grenze: Bei zwölf Phrasen darf auch vor der
    // zehnten noch geschnitten werden — dahinter bleiben zwei, und genau dort
    // fängt bei einem gewöhnlichen Aufbau das Outro an.
    for i in MIN_PHRASEN..=phrasen.saturating_sub(MIN_PHRASEN) {
        if neuheit[i] < SCHWELLE {
            continue;
        }
        // Nur die Spitze, nicht die ganze Flanke.
        if neuheit[i] < neuheit[i - 1] || neuheit[i] < neuheit[i + 1] {
            continue;
        }
        if i - letzte < MIN_PHRASEN {
            continue;
        }
        aus.push(i);
        letzte = i;
    }
    aus
}

fn zusammenfassen(
    fenster: &[(u64, u64)],
    m: &Merkmale,
    grenzen: &[usize],
    sample_rate: u32,
    grid: Beatgrid,
) -> Vec<Abschnitt> {
    let je_beat = 60.0 / grid.bpm as f64 * sample_rate as f64;
    let beat = |frames: u64| (frames as f64 - grid.anchor_frames as f64) / je_beat;

    let spitze = |werte: &[f32]| werte.iter().copied().fold(0.0f32, f32::max).max(1e-9);
    let (p_max, b_max, d_max) = (spitze(&m.pegel), spitze(&m.bass), spitze(&m.dichte));

    let mut kanten = vec![0usize];
    kanten.extend_from_slice(grenzen);
    kanten.push(fenster.len());

    kanten
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let schnitt = |werte: &[f32]| werte[a..b].iter().sum::<f32>() / (b - a) as f32;
            Abschnitt {
                von_frames: fenster[a].0,
                bis_frames: fenster[b - 1].1,
                von_beat: beat(fenster[a].0),
                bis_beat: beat(fenster[b - 1].1),
                art: Art::Teil,
                pegel: schnitt(&m.pegel) / p_max,
                bass: schnitt(&m.bass) / b_max,
                dichte: schnitt(&m.dichte) / d_max,
            }
        })
        .collect()
}

/// Gibt den Abschnitten ihre Namen.
///
/// Alle Schwellen sind **Quantile des Tracks selbst**. Eine absolute Zahl wäre
/// eine Behauptung über alle Musik; ein Quantil ist eine über diesen Track, und
/// die lässt sich wenigstens einhalten.
/// Gibt den Abschnitten ihre Namen.
///
/// **Die Quantile werden über die Abschnitte gebildet, nicht über die
/// Phrasen.** Das klingt nach einer Feinheit und ist keine: Verglichen wird ein
/// Abschnitts*mittel*, und ein Mittel liegt immer unter der lautesten Phrase
/// darin. Bei einem Track, der die meiste Zeit oben ist, rutscht der
/// Phrasen-Median damit fast auf die Spitze — und *jeder* Abschnitt fällt
/// darunter. Ein Stück mit 90 Sekunden Vollgas wurde so zu „Intro, Break,
/// Outro": als wäre es durchweg leise gewesen.
///
/// Gefunden beim ersten Lauf gegen ein Stück ohne inneren Kontrast, nicht im
/// Test — dieselbe Falle wie schon dreimal: eine Schwelle, geeicht an Material,
/// das zufällig die richtige Verteilung hatte.
fn benennen(abschnitte: &mut [Abschnitt]) {
    let q = |werte: &[f32], anteil: f32| {
        let mut s = werte.to_vec();
        s.sort_by(f32::total_cmp);
        let spitze = s.last().copied().unwrap_or(1.0).max(1e-9);
        s[((s.len() - 1) as f32 * anteil) as usize] / spitze
    };
    let pegel: Vec<f32> = abschnitte.iter().map(|a| a.pegel).collect();
    let bass: Vec<f32> = abschnitte.iter().map(|a| a.bass).collect();
    if pegel.is_empty() {
        return;
    }
    let p50 = q(&pegel, 0.5);
    let b75 = q(&bass, 0.75);

    let letzter = abschnitte.len().saturating_sub(1);
    for (i, a) in abschnitte.iter_mut().enumerate() {
        a.art = if i == 0 && a.pegel < p50 {
            Art::Intro
        } else if i == letzter && i > 0 && a.pegel < p50 {
            Art::Outro
        } else if a.bass >= b75 && a.pegel >= p50 {
            Art::Drop
        } else if a.pegel < p50 {
            // Ein Einbruch in der Mitte. Der erste Entwurf verlangte dafür
            // „Bass weg", und das war falsch gemessen: Ein Kick allein bringt
            // schon reichlich Energie unter 160 Hz, ein Break mit laufenden
            // Drums bleibt also im Bassband deutlich sichtbar. Am gebauten
            // Material lag er bei 0,41 gegen 0,99 im Drop — leiser, aber weit
            // entfernt von „weg". Der Pegel trennt das zuverlässiger.
            //
            // Damit ist ein Break dasselbe wie ein Intro, nur nicht am Anfang.
            // Das ist keine Schwäche der Regel, sondern die Sache selbst.
            Art::Break
        } else {
            Art::Teil
        };
    }

    // Ein Aufbau ist nur einer, wenn danach etwas kommt, worauf er aufbaut.
    // Deshalb ein zweiter Durchgang: Beim ersten war der nächste Name noch
    // nicht vergeben.
    for i in 0..abschnitte.len().saturating_sub(1) {
        let naechster_drop = abschnitte[i + 1].art == Art::Drop;
        let a = &mut abschnitte[i];
        if naechster_drop && matches!(a.art, Art::Teil) {
            a.art = Art::Aufbau;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::onset_envelope;

    const RATE: u32 = 44_100;
    const BPM: f32 = 120.0;

    /// Baut Material aus Teilen zu je zwei Phrasen.
    ///
    /// Je Teil: läuft ein Kick, liegt ein Bass darunter, wie laut der Akkord
    /// ist. Bei 120 BPM ist eine Phrase acht Sekunden.
    fn bauen(teile: &[(bool, bool, f32)]) -> Vec<f32> {
        let je_beat = 60.0 / BPM as f64 * RATE as f64;
        let je_phrase = (je_beat * PHRASE_BEATS) as usize;
        let mut mono = Vec::new();

        for &(kick, bass, akkord) in teile {
            let von = mono.len();
            mono.resize(von + je_phrase * 2, 0.0);
            let teil = &mut mono[von..];

            for (i, wert) in teil.iter_mut().enumerate() {
                // Die Zeit läuft über das ganze Stück weiter, sonst entstünde
                // an jeder Teilgrenze ein Phasensprung — also ein Onset, den
                // die Gliederung dann zu Recht fände.
                let t = (von + i) as f32 / RATE as f32;
                for hz in [220.0f32, 277.18, 329.63] {
                    *wert += (std::f32::consts::TAU * hz * t).sin() * akkord / 3.0;
                }
                if bass {
                    *wert += (std::f32::consts::TAU * 55.0 * t).sin() * 0.5;
                }
            }

            if kick {
                let schlag = je_beat as usize;
                let mut start = 0;
                while start + RATE as usize / 4 < teil.len() {
                    for i in 0..RATE as usize / 4 {
                        let t = i as f32 / RATE as f32;
                        let f = 110.0 * (-t * 30.0).exp() + 45.0;
                        teil[start + i] +=
                            (std::f32::consts::TAU * f * t).sin() * (-t * 22.0).exp() * 0.9;
                    }
                    start += schlag;
                }
            }
        }

        mono.iter().flat_map(|v| [*v, *v]).collect()
    }

    /// Ein Track mit bekannter Gliederung: Intro (Akkord, keine Drums), Aufbau
    /// (Drums ohne Bass), Drop (alles), Break (Drums, leiser, kein Bass), Drop,
    /// Outro (nur Akkord). Die Grenzen liegen bei Beat 32, 64, 96, 128, 160.
    fn gebauter_track() -> Vec<f32> {
        bauen(&[
            (false, false, 0.30),
            (true, false, 0.30),
            (true, true, 0.30),
            (true, false, 0.12),
            (true, true, 0.30),
            (false, false, 0.20),
        ])
    }

    fn gliedern(samples: &[f32]) -> Struktur {
        let env = onset_envelope(samples, RATE);
        let grid = Beatgrid {
            bpm: BPM,
            anchor_frames: 0,
            confidence: 1.0,
        };
        analysiere(samples, RATE, grid, &env).expect("keine Gliederung")
    }

    /// Die Grenzen sitzen dort, wo die Teile wechseln — auf ±1 Phrase.
    ///
    /// Genauer geht es nicht: Der Kern mittelt über zwei Phrasen je Seite, und
    /// genau das macht ihn gegen einzelne Variationen unempfindlich.
    #[test]
    fn die_grenzen_liegen_an_den_gebauten_uebergaengen() {
        let s = gliedern(&gebauter_track());
        let phrase = PHRASE_BEATS;

        let grenzen: Vec<f64> = s.abschnitte.iter().skip(1).map(|a| a.von_beat).collect();
        assert!(
            grenzen.len() >= 4,
            "nur {} Grenzen gefunden: {:?}",
            grenzen.len(),
            s.abschnitte.iter().map(|a| a.von_beat).collect::<Vec<_>>()
        );

        // Gebaut wurde bei Beat 32, 64, 96, 128, 160.
        for erwartet in [32.0, 64.0, 96.0, 128.0, 160.0] {
            let naechste = grenzen
                .iter()
                .map(|g| (g - erwartet).abs())
                .fold(f64::MAX, f64::min);
            assert!(
                naechste <= phrase,
                "bei Beat {erwartet} fehlt eine Grenze (nächste {naechste:.0} Beats entfernt): \
{grenzen:?}"
            );
        }
    }

    /// Die ganze Kette, gegen bekannte Wahrheit.
    ///
    /// Das ist der eigentliche Prüfstein von S2: sechs gebaute Teile, sechs
    /// erkannte, mit den richtigen Namen in der richtigen Reihenfolge.
    #[test]
    fn die_gliederung_findet_die_gebauten_teile_mit_namen() {
        let s = gliedern(&gebauter_track());
        let arten: Vec<Art> = s.abschnitte.iter().map(|a| a.art).collect();
        assert_eq!(
            arten,
            vec![
                Art::Intro,
                Art::Aufbau,
                Art::Drop,
                Art::Break,
                Art::Drop,
                Art::Outro
            ],
            "{:?}",
            s.abschnitte
                .iter()
                .map(|a| (a.art, a.von_beat, a.pegel, a.bass))
                .collect::<Vec<_>>()
        );
        assert!(s.outro_frames().is_some(), "das Outro fehlt");
    }

    /// Ein Track endet selten glatt auf einer Phrasengrenze. Was hinter der
    /// letzten vollen liegt, ist genau das Outro — es darf nicht wegfallen.
    #[test]
    fn der_rest_hinter_der_letzten_vollen_phrase_zaehlt_mit() {
        let mut samples = gebauter_track();
        // Eine Dreivierteilphrase mehr, wie sie beim Ausblenden entsteht.
        let je_beat = 60.0 / BPM as f64 * RATE as f64;
        let anhang = (je_beat * PHRASE_BEATS * 0.75) as usize * CHANNELS;
        let letzter = samples[samples.len() - anhang..].to_vec();
        samples.extend_from_slice(&letzter);

        let s = gliedern(&samples);
        let ende = s.abschnitte.last().expect("kein Abschnitt").bis_frames;
        assert_eq!(
            ende as usize,
            samples.len() / CHANNELS,
            "der Schwanz des Tracks fehlt in der Gliederung"
        );
    }

    /// **Ein Track, der die meiste Zeit oben ist, ist nicht durchweg leise.**
    ///
    /// Gefunden beim ersten Lauf gegen so ein Stück, nicht im Test: Ein kurzes
    /// Intro, sechs Phrasen Vollgas, ein kurzes Outro — und die Mitte hieß
    /// `break`. Der Grund war, dass die Quantile über *Phrasen* gebildet
    /// wurden, verglichen aber mit Abschnitts*mitteln*. Ein Mittel liegt immer
    /// unter der lautesten Phrase darin; ist die Mehrheit der Phrasen laut,
    /// liegt der Median fast auf der Spitze, und dann fällt jeder Abschnitt
    /// darunter.
    ///
    /// Das ist beim gebauten Prüfmaterial nie aufgefallen, weil dort laute und
    /// leise Teile sich abwechseln und der Median genau dazwischen landet —
    /// dieselbe Falle wie bei der Tempo-Schwelle an Klick-Tracks.
    #[test]
    fn ein_stueck_das_meist_oben_ist_heisst_nicht_break() {
        // Kurzes Intro, sechs Phrasen laut, kurzes Outro. **Die lauten Teile
        // sind nicht exakt gleich laut** — bei perfekt gleichen Phrasen fällt
        // der Fehler nicht auf, weil das Abschnittsmittel dann genau auf dem
        // Median liegt und die Prüfung `>=` gerade noch hält. Genau daran ist
        // der erste Anlauf dieses Tests vorbeigelaufen.
        let s = gliedern(&bauen(&[
            (false, false, 0.20),
            (true, true, 0.34),
            (true, true, 0.28),
            (true, true, 0.31),
            (true, true, 0.29),
            (true, true, 0.33),
            (true, true, 0.30),
            (false, false, 0.20),
        ]));

        let mitte: Vec<Art> = s
            .abschnitte
            .iter()
            .filter(|a| a.pegel > 0.8)
            .map(|a| a.art)
            .collect();
        assert!(
            !mitte.is_empty(),
            "kein lauter Abschnitt: {:?}",
            s.abschnitte
        );
        assert!(
            mitte.iter().all(|a| *a == Art::Drop),
            "der laute Teil heißt {mitte:?}, nicht Drop: {:?}",
            s.abschnitte
                .iter()
                .map(|a| (a.art, a.pegel, a.bass))
                .collect::<Vec<_>>()
        );
    }

    /// Der Anfang ist leise und ohne Schläge — das ist ein Intro.
    #[test]
    fn der_leise_anfang_heisst_intro() {
        let s = gliedern(&gebauter_track());
        assert_eq!(s.abschnitte[0].art, Art::Intro, "{:?}", s.abschnitte[0]);
        assert!(s.intro_beats().is_some_and(|b| b >= PHRASE_BEATS));
    }

    /// Bass da und laut — das ist ein Drop. Und mindestens einer muss dabei
    /// sein, sonst hat die Benennung nichts erkannt.
    #[test]
    fn wo_der_bass_ist_steht_ein_drop() {
        let s = gliedern(&gebauter_track());
        let drops: Vec<&Abschnitt> = s.abschnitte.iter().filter(|a| a.art == Art::Drop).collect();
        assert!(
            !drops.is_empty(),
            "kein Drop erkannt: {:?}",
            s.abschnitte
                .iter()
                .map(|a| (a.art, a.bass))
                .collect::<Vec<_>>()
        );
        for d in &drops {
            assert!(d.bass > 0.5, "ein Drop ohne Bass: {d:?}");
        }
    }

    /// Der Einstiegspunkt ist die erste Eins, nicht Frame 0.
    #[test]
    fn der_einstieg_liegt_auf_einer_phrasengrenze() {
        let samples = gebauter_track();
        let env = onset_envelope(&samples, RATE);
        // Ein Anker mitten in der Phrase: Der Vorlauf davor gehört zu keiner.
        let grid = Beatgrid {
            bpm: BPM,
            anchor_frames: (0.75 * RATE as f64) as u64,
            confidence: 1.0,
        };
        let s = analysiere(&samples, RATE, grid, &env).expect("keine Gliederung");

        let einstieg = s.einstieg_frames().expect("kein Einstieg");
        let je_beat = 60.0 / BPM as f64 * RATE as f64;
        let beat = (einstieg as f64 - grid.anchor_frames as f64) / je_beat;
        assert!(
            (beat.rem_euclid(PHRASE_BEATS)).abs() < 0.05,
            "der Einstieg liegt bei Beat {beat:.2}, nicht auf einer Phrasengrenze"
        );
    }

    /// Aus zwei Phrasen lässt sich keine Gliederung lesen. Dann lieber nichts
    /// sagen als etwas erfinden.
    #[test]
    fn ein_zu_kurzer_track_bekommt_keine_gliederung() {
        let samples: Vec<f32> = vec![0.1; RATE as usize * 2 * CHANNELS];
        let env = onset_envelope(&samples, RATE);
        let grid = Beatgrid {
            bpm: BPM,
            anchor_frames: 0,
            confidence: 1.0,
        };
        assert!(analysiere(&samples, RATE, grid, &env).is_none());
    }

    /// Ohne Tempo gibt es kein Phrasengitter — und damit keine Gliederung.
    #[test]
    fn ohne_tempo_keine_gliederung() {
        let samples = gebauter_track();
        let env = onset_envelope(&samples, RATE);
        let grid = Beatgrid {
            bpm: 0.0,
            anchor_frames: 0,
            confidence: 0.0,
        };
        assert!(analysiere(&samples, RATE, grid, &env).is_none());
    }

    /// Gleichförmiges Material hat keine Abschnitte — und darf sich keine
    /// ausdenken.
    ///
    /// **Der erste Anlauf nahm dafür einen reinen Sinus.** Der ist als Prüfstein
    /// unbrauchbar: Sein spektraler Fluss driftet über das Stück um 45 %, weil
    /// die Bin-Leckage gegen den Hop schwebt. Damit hätte der Test eine
    /// Robustheit gegen etwas verlangt, das in Musik nicht vorkommt — dieselbe
    /// Falle wie bei der Tempo-Schwelle, die an Klick-Tracks geeicht war. Hier
    /// steht jetzt ein durchlaufender Takt: Kick, Bass, Akkord, zwölf Phrasen
    /// lang unverändert.
    #[test]
    fn gleichfoermiges_material_zerfaellt_nicht() {
        let samples = bauen(&[(true, true, 0.30); 6]);
        let s = gliedern(&samples);
        assert_eq!(
            s.abschnitte.len(),
            1,
            "aus gleichförmigem Material wurden {} Abschnitte: {:?}",
            s.abschnitte.len(),
            s.abschnitte.iter().map(|a| a.von_beat).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ein_abschnitt_laesst_sich_ueber_den_frame_finden() {
        let s = gliedern(&gebauter_track());
        let erster = s.abschnitte[0];
        assert_eq!(s.bei_frames(erster.von_frames), Some(&erster));
        assert_eq!(s.bei_frames(erster.bis_frames - 1), Some(&erster));
        assert!(s.bei_frames(u64::MAX).is_none());
    }

    #[test]
    fn jede_art_ueberlebt_ihren_namen() {
        for art in [
            Art::Intro,
            Art::Aufbau,
            Art::Drop,
            Art::Break,
            Art::Outro,
            Art::Teil,
        ] {
            assert_eq!(Art::parse(art.name()), Some(art));
        }
        assert_eq!(Art::parse("refrain"), None);
    }
}
