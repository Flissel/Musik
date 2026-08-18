//! Synthetische Tracks für den Fall ohne Musiksammlung.
//!
//! Damit lässt sich die Oberfläche starten und beurteilen, ohne dass Dateien
//! vorliegen müssen — und sie zeigt dabei echte Wellenformen, ein echtes
//! Beatgrid und eine echte Tonart, weil das Material durch dieselbe Analyse
//! läuft wie alles andere.
//!
//! Deshalb liegen über den Drums auch Akkorde und nicht nur ein Bass: Aus
//! Bass und Schlagzeug allein ermittelt die Analyse **keine** Tonart, und das
//! zu Recht (siehe `analysis::tonart`). Ohne Akkorde könnte die Demo die halbe
//! Anzeige nicht vorführen.
//!
//! Die beiden Decks stehen in a-Moll (8A) und e-Moll (9A) — auf dem
//! Camelot-Rad Nachbarn, also ein Paar, das sich harmonisch mischen lässt.

use std::f32::consts::PI;

use audio_core::Track;

pub struct DemoTrack {
    pub track: Track,
    pub artist: String,
    pub title: String,
}

pub fn deck_a(sample_rate: u32) -> DemoTrack {
    DemoTrack {
        track: bauen(sample_rate, 128.0, 16, muster_a),
        artist: "Demo".into(),
        title: "Vier auf die Eins · 128 · Am".into(),
    }
}

pub fn deck_b(sample_rate: u32) -> DemoTrack {
    DemoTrack {
        track: bauen(sample_rate, 124.0, 16, muster_b),
        artist: "Demo".into(),
        title: "Snare-Muster · 124 · Em".into(),
    }
}

/// Die Akkordfolgen der beiden Demos, als Halbtöne über C4.
///
/// Deck A: Am – Am – Dm – C, also a-Moll. Deck B: Em – Em – Am – G, also
/// e-Moll. Der Bass nimmt jeweils den Grundakkord eine bzw. zwei Oktaven
/// tiefer.
const STUFEN_A: [[i32; 3]; 4] = [[9, 12, 16], [9, 12, 16], [2, 5, 9], [0, 4, 7]];
const STUFEN_B: [[i32; 3]; 4] = [[4, 7, 11], [4, 7, 11], [9, 12, 16], [7, 11, 14]];

fn muster_a(bar: usize, step: usize, add: &mut dyn FnMut(usize, f32), rate: u32) {
    if step.is_multiple_of(4) {
        kick(add, rate);
    }
    if step % 4 == 2 {
        hat(add, rate, 0.22);
    }
    let akkord = STUFEN_A[bar % 4];
    if step.is_multiple_of(2) {
        bass(add, hz(akkord[0] - 24), rate, 0.30);
    }
    // Einmal je Takt, dafür lang: Ein Akkord, der auf jedem Sechzehntel neu
    // anschlägt, wäre ein Stakkato und kein Flächenklang.
    if step == 0 {
        flaeche(add, &akkord, rate, 0.10);
    }
}

fn muster_b(bar: usize, step: usize, add: &mut dyn FnMut(usize, f32), rate: u32) {
    if step.is_multiple_of(4) {
        kick(add, rate);
    }
    if step % 8 == 4 {
        snare(add, rate);
    }
    let akkord = STUFEN_B[bar % 4];
    if step % 2 == 1 {
        bass(add, hz(akkord[0] - 24), rate, 0.26);
    }
    if step == 0 {
        flaeche(add, &akkord, rate, 0.20);
    }
}

/// Halbtöne über C4 in Hertz.
fn hz(halbton_ueber_c4: i32) -> f32 {
    261.63 * 2.0f32.powf(halbton_ueber_c4 as f32 / 12.0)
}

/// Ein Muster füllt einen Sechzehntel-Schritt eines Taktes.
///
/// Die Klänge werden über einen Rückruf addiert statt zurückgegeben, damit
/// sich mehrere Klänge im selben Schritt überlagern können.
type Muster = fn(usize, usize, &mut dyn FnMut(usize, f32), u32);

fn bauen(rate: u32, bpm: f32, bars: usize, muster: Muster) -> Track {
    let frames_je_step = rate as f32 * 60.0 / bpm / 4.0;
    let total = (frames_je_step * 16.0 * bars as f32) as usize;
    let mut mono = vec![0.0f32; total];

    for bar in 0..bars {
        for step in 0..16 {
            let start = ((bar * 16 + step) as f32 * frames_je_step) as usize;
            let mut add = |offset: usize, value: f32| {
                let idx = start + offset;
                if idx < total {
                    mono[idx] += value;
                }
            };
            muster(bar, step, &mut add, rate);
        }
    }

    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
    let scale = 0.75 / peak;

    Track {
        samples: mono
            .into_iter()
            .flat_map(|v| {
                let s = v * scale;
                [s, s]
            })
            .collect(),
        sample_rate: rate,
        stems: Vec::new(),
    }
}

fn kick(add: &mut dyn FnMut(usize, f32), rate: u32) {
    let n = (rate as f32 * 0.25) as usize;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-t * 22.0).exp();
        let f = 110.0 * (-t * 30.0).exp() + 45.0;
        add(i, (2.0 * PI * f * t).sin() * env * 0.9);
    }
}

fn snare(add: &mut dyn FnMut(usize, f32), rate: u32) {
    let n = (rate as f32 * 0.19) as usize;
    let mut seed = 0x2545_F491u32;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-t * 30.0).exp();
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
        add(
            i,
            (noise * 0.6 + (2.0 * PI * 190.0 * t).sin() * 0.4) * env * 0.6,
        );
    }
}

fn hat(add: &mut dyn FnMut(usize, f32), rate: u32, gain: f32) {
    let n = (rate as f32 * 0.05) as usize;
    let mut seed = 0x9E37_79B9u32;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (-t * 120.0).exp();
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
        add(i, noise * env * gain);
    }
}

/// Ein Akkord als Flächenklang — die Harmonik der Demo.
///
/// Jede Stimme bekommt ein paar Obertöne, damit sie im Spektrum breiter steht
/// als ein nackter Sinus. Leise, weil sie unter den Drums liegen soll und
/// nicht darüber.
fn flaeche(add: &mut dyn FnMut(usize, f32), halbtoene: &[i32], rate: u32, gain: f32) {
    let n = (rate as f32 * 1.6) as usize;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        // Weich ein und aus, sonst knackt es und schmiert breitbandig.
        let huelle = (t * 8.0).min(1.0).min((1.6 - t) * 6.0).max(0.0);

        let mut wert = 0.0;
        for halbton in halbtoene {
            let grund = hz(*halbton);
            for (ober, staerke) in [(1.0, 1.0), (2.0, 0.4), (3.0, 0.2)] {
                wert += (2.0 * PI * grund * ober * t).sin() * staerke;
            }
        }
        add(i, wert * huelle * gain / halbtoene.len() as f32);
    }
}

fn bass(add: &mut dyn FnMut(usize, f32), freq: f32, rate: u32, gain: f32) {
    let n = (rate as f32 * 0.3) as usize;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let env = (1.0 - (-t * 120.0).exp()) * (-t * 6.0).exp();
        let saw = 2.0 * ((freq * t) % 1.0) - 1.0;
        add(i, saw * env * gain);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Demo verspricht in ihrem Titel ein Tempo. Findet die Analyse es
    /// nicht wieder, zeigt die Oberfläche „— BPM" und Sync bleibt tot — dann
    /// taugt die Demo nicht als Vorführung.
    #[test]
    fn die_analyse_findet_das_versprochene_tempo() {
        for (demo, erwartet) in [(deck_a(48_000), 128.0), (deck_b(48_000), 124.0)] {
            let analyse = analysis::analyze(&demo.track);
            let bpm = analyse
                .bpm
                .unwrap_or_else(|| panic!("{}: kein Tempo erkannt", demo.title));
            assert!(
                (bpm - erwartet).abs() < 1.0,
                "{}: {bpm} statt {erwartet}",
                demo.title
            );
            assert!(
                analyse.beat_anchor_frames.is_some(),
                "{}: kein Anker",
                demo.title
            );
        }
    }

    /// Dasselbe für die Tonart — und aus demselben Grund.
    ///
    /// Die Demo ist die einzige Stelle, an der sich die Anzeige ohne eigene
    /// Sammlung beurteilen lässt. Zeigt sie hier „—", ist entweder die
    /// Erkennung kaputt oder das Material taugt nicht als Vorführung; beides
    /// will man wissen, bevor jemand das Fenster aufmacht.
    #[test]
    fn die_analyse_findet_die_versprochene_tonart() {
        for (demo, erwartet) in [(deck_a(48_000), "Am"), (deck_b(48_000), "Em")] {
            let analyse = analysis::analyze(&demo.track);
            let tonart = analyse
                .tonart()
                .unwrap_or_else(|| panic!("{}: keine Tonart erkannt", demo.title));
            assert_eq!(tonart.name(), erwartet, "{}", demo.title);
        }
    }

    /// Die beiden Decks sollen sich harmonisch mischen lassen.
    ///
    /// Sonst führt die Demo zwar eine Tonart vor, aber nicht, wozu sie da ist.
    #[test]
    fn die_beiden_demo_decks_passen_harmonisch_zueinander() {
        let a = analysis::analyze(&deck_a(48_000).track).tonart().unwrap();
        let b = analysis::analyze(&deck_b(48_000).track).tonart().unwrap();

        assert!(
            a.passt_zu(&b),
            "{} ({}) passt nicht zu {} ({})",
            a.name(),
            a.camelot(),
            b.name(),
            b.camelot()
        );
    }
}
