//! Testsignale für die Analyse.

use audio_core::track::CHANNELS;

/// Samplerate der harmonischen Testsignale.
const TON_RATE: u32 = 44_100;

/// Ein Ton mit ein paar Obertönen — ein nackter Sinus wäre unrealistisch
/// einfach, weil er genau einen Bin trifft.
fn ton(halbton_ueber_c4: i32, sekunden: f32, ziel: &mut Vec<f32>) {
    let grund = 261.63 * 2.0f32.powf(halbton_ueber_c4 as f32 / 12.0);
    let n = (TON_RATE as f32 * sekunden) as usize;
    let vorher = ziel.len() / CHANNELS;

    ziel.resize((vorher + n) * CHANNELS, 0.0);
    for i in 0..n {
        let t = i as f32 / TON_RATE as f32;
        let mut wert = 0.0;
        for (ober, staerke) in [(1.0, 1.0), (2.0, 0.5), (3.0, 0.3), (4.0, 0.15)] {
            wert += (std::f32::consts::TAU * grund * ober * t).sin() * staerke;
        }
        // Weich ein- und ausblenden, sonst erzeugt der Sprung Breitband.
        let huelle = (t * 20.0).min(1.0).min((sekunden - t) * 20.0).max(0.0);
        let v = wert * huelle * 0.2;
        let platz = (vorher + i) * CHANNELS;
        ziel[platz] = v;
        ziel[platz + 1] = v;
    }
}

/// Ein Beat mit Kick und einer Bassfigur — der schwere Fall für die Tonart.
///
/// So klingt Clubmusik: Die ganze Energie liegt unten, harmonisch trägt allein
/// der Bass, und der Kick schmiert breitbandig darüber. Genau daran ist die
/// Erkennung zuerst gescheitert, deshalb steht das Material hier und nicht als
/// Zierde in einem einzelnen Test.
///
/// `noten` sind Halbtöne über C2 (65,4 Hz), je einen Takt lang.
pub fn bass_mit_kick(noten: &[i32], takte: usize) -> Vec<f32> {
    const RATE: f32 = 44_100.0;
    let je_takt = (RATE * 2.0) as usize;
    let mut mono = vec![0.0f32; je_takt * takte];

    for takt in 0..takte {
        let note = noten[takt % noten.len()];
        let hz = 65.41 * 2.0f32.powf(note as f32 / 12.0);

        for schlag in 0..4 {
            let start = takt * je_takt + schlag * je_takt / 4;

            // Kick: ein Sinus, der von 110 Hz nach unten rutscht.
            for i in 0..(RATE * 0.25) as usize {
                if start + i >= mono.len() {
                    break;
                }
                let t = i as f32 / RATE;
                let f = 110.0 * (-t * 30.0).exp() + 45.0;
                mono[start + i] += (std::f32::consts::TAU * f * t).sin() * (-t * 22.0).exp() * 0.9;
            }

            // Bass: ein Sägezahn, also alle Teiltöne mit 1/n.
            for i in 0..(RATE * 0.4) as usize {
                if start + i >= mono.len() {
                    break;
                }
                let t = i as f32 / RATE;
                let huelle = (1.0 - (-t * 120.0).exp()) * (-t * 5.0).exp();
                mono[start + i] += (2.0 * ((hz * t) % 1.0) - 1.0) * huelle * 0.3;
            }
        }
    }

    mono.into_iter().flat_map(|v| [v, v]).collect()
}

/// Eine Akkordfolge in einer Tonart, um `versatz` Halbtöne verschoben.
///
/// **Mit Bass und mit längerem Grundakkord**, und das ist nicht Zierde:
/// Dur und die parallele Molltonart bestehen aus denselben zwölf Tönen.
/// Was sie unterscheidet, ist die Gewichtung — welcher Ton trägt, wie
/// lange der Grundakkord steht, was im Bass liegt. Eine Folge mit
/// gleichlangen Akkorden und ohne Bass ist deshalb kein Prüfstein,
/// sondern eine Fangfrage: Sie hat schlicht keine eindeutige Tonart.
pub fn akkordfolge(versatz: i32, moll: bool) -> Vec<f32> {
    // I–V–vi–IV in Dur, i–VI–III–VII in Moll — beides Allerweltsfolgen.
    // Die Zahl dahinter ist die Länge in Sekunden; der Grundakkord steht
    // länger, wie in echter Musik auch.
    let stufen: &[(&[i32], f32)] = if moll {
        &[
            (&[0, 3, 7], 2.0),
            (&[8, 12, 15], 1.0),
            (&[3, 7, 10], 1.0),
            (&[10, 14, 17], 1.0),
        ]
    } else {
        &[
            (&[0, 4, 7], 2.0),
            (&[7, 11, 14], 1.0),
            (&[9, 12, 16], 1.0),
            (&[5, 9, 12], 1.0),
        ]
    };

    let mut aus = Vec::new();
    for _ in 0..2 {
        for (akkord, dauer) in stufen {
            let start = aus.len();
            // Bass auf dem Grundton, eine Oktave tiefer — er sagt dem Ohr,
            // welcher Ton der Bezugspunkt ist.
            let mut stimmen: Vec<i32> = vec![akkord[0] - 12];
            stimmen.extend_from_slice(akkord);

            for note in stimmen {
                let mut stimme = Vec::new();
                ton(note + versatz, *dauer, &mut stimme);
                if aus.len() < start + stimme.len() {
                    aus.resize(start + stimme.len(), 0.0);
                }
                for (i, v) in stimme.iter().enumerate() {
                    aus[start + i] += v;
                }
            }
        }
    }
    aus
}

/// Erzeugt einen Klick-Track als interleaved Stereo.
///
/// Die Klicks sind kurze, deterministisch erzeugte Rauschstöße mit
/// exponentiellem Abfall — breitbandig genug, damit der spektrale Fluss sie
/// deutlich sieht, und ohne Abhängigkeit auf einen Zufallsgenerator.
pub fn click_track(bpm: f64, sample_rate: u32, secs: f64, offset_secs: f64) -> Vec<f32> {
    let frames = (sample_rate as f64 * secs) as usize;
    let mut out = vec![0.0f32; frames * CHANNELS];

    let period = 60.0 / bpm * sample_rate as f64;
    let burst = ((sample_rate as f64 * 0.008) as usize).max(1);
    let decay = burst as f32 * 0.25;

    let mut seed = 0x1234_5678u32;
    let mut at = offset_secs * sample_rate as f64;

    while (at as usize) < frames {
        let start = at as usize;
        for i in 0..burst {
            let idx = start + i;
            if idx >= frames {
                break;
            }
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
            let env = (-(i as f32) / decay).exp();
            let v = noise * env * 0.8;
            out[idx * CHANNELS] += v;
            out[idx * CHANNELS + 1] += v;
        }
        at += period;
    }

    out
}
