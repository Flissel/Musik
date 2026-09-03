//! Rendert einen Mix offline in eine WAV-Datei.
//!
//! Zwei Zwecke. Erstens lässt sich die Engine damit ohne Audiogerät prüfen —
//! derselbe Signalweg, nur schneller als Echtzeit und in eine Datei statt auf
//! die Anlage. Zweitens kann man das Ergebnis hinterher anhören, was in einer
//! Umgebung ohne Soundkarte der einzige Weg ist, überhaupt etwas zu beurteilen.
//!
//! Der Übergang ist ein Bass-Swap, wie man ihn von Hand fahren würde: Der
//! Crossfader wandert hinüber, und in der Mitte wechselt der Bass von einem
//! Deck zum anderen. Zwei Bässe gleichzeitig matschen — deshalb macht das
//! niemand anders.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use audio_core::Track;
use audio_core::deck::{DeckState, Voice};
use audio_engine::{Assign, DeckSource, Engine, Source, aux_channel};

const BLOCK: usize = 512;

struct Options {
    out: PathBuf,
    /// Wohin der Kopfhörer-Bus geschrieben wird.
    ///
    /// Ohne Angabe gar nicht — dann rendert der Mixer wie bisher zweikanalig.
    /// Mit Angabe läuft er vierkanalig, also über denselben Weg, den ein Gerät
    /// mit vier Ausgängen nimmt.
    cue_out: Option<PathBuf>,
    a: Option<PathBuf>,
    b: Option<PathBuf>,
    aux: Option<PathBuf>,
    seconds: f64,
    transition: Option<f64>,
    transition_len: f64,
    sample_rate: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            out: PathBuf::from("mix.wav"),
            cue_out: None,
            a: None,
            b: None,
            aux: None,
            seconds: 60.0,
            transition: None,
            transition_len: 16.0,
            sample_rate: 48_000,
        }
    }
}

fn main() -> Result<()> {
    let opts = parse_args()?;
    let rate = opts.sample_rate;

    // Deck B wird gestreckt und braucht dadurch mehr Quellmaterial als
    // Renderzeit — mit Reserve.
    let reserve = opts.seconds * 1.5 + 10.0;
    let (track_a, quelle_a) =
        lade_oder_synthetisiere(opts.a.as_deref(), rate, Variante::A, reserve)?;
    let (track_b, quelle_b) =
        lade_oder_synthetisiere(opts.b.as_deref(), rate, Variante::B, reserve)?;

    let mut engine = Engine::new(rate as f32);

    let deck_a = engine.add_channel("A", quelle_a);
    let deck_b = engine.add_channel("B", quelle_b);
    engine.channel(deck_a).set_assign(Assign::A);
    engine.channel(deck_b).set_assign(Assign::B);

    // Tempo angleichen, sofern beide Seiten ein Grid haben.
    let tempo_info = angleichen(&track_a, &track_b);

    // AUX läuft auf Thru — der Crossfader darf ihn nicht wegnehmen.
    let aux_kanal = if let Some(pfad) = opts.aux.as_deref() {
        let track = Track::decode_file(pfad)
            .with_context(|| format!("AUX: {} nicht lesbar", pfad.display()))?
            .resampled_to(rate);
        let (mut writer, source) = aux_channel(rate as usize * 4);
        writer.write(&track.samples);

        let idx = engine.add_channel("AUX", Box::new(source));
        engine.channel(idx).set_assign(Assign::Thru);
        engine.channel(idx).set_fader(0.35);
        Some(idx)
    } else {
        None
    };

    let transition = opts.transition.unwrap_or(opts.seconds * 0.5);
    let frames_total = (opts.seconds * rate as f64) as usize;

    println!("Rendere {:.0}s @ {} Hz", opts.seconds, rate);
    println!("  Deck A: {}", beschreibung(opts.a.as_deref(), Variante::A));
    println!("  Deck B: {}", beschreibung(opts.b.as_deref(), Variante::B));
    if let Some(idx) = aux_kanal {
        println!("  AUX:    {} (Thru, Fader 0.35)", engine.channel(idx).name);
    }
    if let Some(text) = &tempo_info {
        println!("  {text}");
    }
    println!(
        "  Übergang ab {transition:.1}s über {:.0}s",
        opts.transition_len
    );

    // Mit `--cue` läuft der Mixer vierkanalig — derselbe Weg, den ein Gerät mit
    // vier Ausgängen nimmt. Ohne bleibt es beim Stereo-Pfad, damit ein
    // gewöhnlicher Mixdown nicht die doppelte Arbeit macht.
    let kanaele = if opts.cue_out.is_some() { 4 } else { 2 };
    if let Some(pfad) = &opts.cue_out {
        // Ohne Cue-Taste bliebe die Datei still — dann sähe eine Trennung, die
        // nicht funktioniert, genauso aus wie eine, die funktioniert.
        engine.channel(deck_b).set_cue(true);
        println!("  Cue:    {} (Deck B liegt vor)", pfad.display());
    }

    let mut pcm: Vec<f32> = Vec::with_capacity(frames_total * 2);
    let mut cue_pcm: Vec<f32> = Vec::new();
    let mut block = vec![0.0f32; BLOCK * kanaele];
    let mut frame = 0usize;

    while frame < frames_total {
        let n = BLOCK.min(frames_total - frame);
        let zeit = frame as f64 / rate as f64;

        automatisieren(
            &mut engine,
            deck_a,
            deck_b,
            zeit,
            transition,
            opts.transition_len,
        );

        let slice = &mut block[..n * kanaele];
        engine.render(slice, kanaele);

        if kanaele == 2 {
            pcm.extend_from_slice(slice);
        } else {
            aufteilen(slice, kanaele, &mut pcm, &mut cue_pcm);
        }

        frame += n;
    }

    schreibe_wav(&opts.out, &pcm, rate).context("WAV konnte nicht geschrieben werden")?;
    println!(
        "\n{} geschrieben ({:.1} MB)",
        opts.out.display(),
        (pcm.len() * 2) as f64 / 1_048_576.0
    );

    if let Some(pfad) = &opts.cue_out {
        schreibe_wav(pfad, &cue_pcm, rate).context("Cue-WAV konnte nicht geschrieben werden")?;
        println!(
            "{} geschrieben ({:.1} MB) — muss anders klingen als die Summe",
            pfad.display(),
            (cue_pcm.len() * 2) as f64 / 1_048_576.0
        );
    }

    Ok(())
}

/// Trennt einen vierkanaligen Block in Summe und Kopfhörer.
///
/// 0/1 sind die Summe, 2/3 der Kopfhörer — genau die Belegung, die auch am
/// Gerät herauskommt. Vertauscht wäre sie hörbar falsch, aber erst beim
/// Anhören; deshalb steht sie in einer eigenen Funktion mit eigenem Test.
fn aufteilen(block: &[f32], kanaele: usize, summe: &mut Vec<f32>, kopfhoerer: &mut Vec<f32>) {
    for f in block.chunks_exact(kanaele) {
        summe.extend_from_slice(&f[..2]);
        kopfhoerer.extend_from_slice(&f[2..4]);
    }
}

/// Fahrplan des Übergangs.
///
/// Vor dem Übergang läuft A allein, danach B allein. Dazwischen wandert der
/// Crossfader hinüber, und der Bass wechselt in der Mitte die Seite.
fn automatisieren(
    engine: &mut Engine,
    deck_a: usize,
    deck_b: usize,
    zeit: f64,
    start: f64,
    dauer: f64,
) {
    let t = ((zeit - start) / dauer).clamp(0.0, 1.0) as f32;

    engine.channel(deck_a).set_fader(1.0);
    engine.channel(deck_b).set_fader(1.0);
    engine.crossfader().set_position(t * 2.0 - 1.0);

    // Bass-Swap: B bringt seinen Bass erst nach der Hälfte, A gibt ihn dort ab.
    let (bass_a, bass_b) = if t < 0.5 {
        (1.0, (t * 2.0).powi(2) * 0.0)
    } else {
        let u = (t - 0.5) * 2.0;
        (1.0 - u, u)
    };

    engine.channel(deck_a).set_eq(bass_a, 1.0, 1.0);
    engine.channel(deck_b).set_eq(bass_b, 1.0, 1.0);

    // Gegen Ende des Übergangs ein kurzer Hochpass auf A — nimmt ihm das
    // Fundament, bevor er ganz verschwindet.
    let filter_a = if t > 0.6 {
        ((t - 0.6) / 0.4).clamp(0.0, 1.0) * 0.7
    } else {
        0.0
    };
    engine.channel(deck_a).set_filter(filter_a);
}

/// Zieht Deck B auf das Tempo von Deck A.
///
/// Das ist der Grund, warum auch die synthetischen Loops als echte Decks
/// laufen und nicht als simple Schleifen: Nur so greift der Zeitstrecker, und
/// nur dann stimmt auch die Meldung darüber.
fn angleichen(a: &Deck, b: &Deck) -> Option<String> {
    let (bpm_a, bpm_b) = (a.bpm?, b.bpm?);
    let ratio = bpm_a / bpm_b;
    b.state.set_tempo(ratio);

    Some(format!(
        "Tempo: B {bpm_b:.2} → {:.2} BPM (Faktor {:.4}, Keylock an)",
        bpm_b * b.state.tempo(),
        b.state.tempo()
    ))
}

struct Deck {
    bpm: Option<f32>,
    state: Arc<DeckState>,
}

enum Variante {
    A,
    B,
}

fn beschreibung(pfad: Option<&Path>, variante: Variante) -> String {
    match pfad {
        Some(p) => p.display().to_string(),
        None => match variante {
            Variante::A => "synthetisch, 128 BPM (Kick, Bass, Hats)".into(),
            Variante::B => "synthetisch, 124 BPM (Kick, Bass, Snare)".into(),
        },
    }
}

/// Lädt eine Datei oder erzeugt einen synthetischen Loop, und hängt beides an
/// ein Deck.
fn lade_oder_synthetisiere(
    pfad: Option<&Path>,
    rate: u32,
    variante: Variante,
    mindestlaenge: f64,
) -> Result<(Deck, Box<dyn Source>)> {
    let (track, bpm) = match pfad {
        Some(p) => {
            let track = Track::decode_file(p)
                .with_context(|| format!("{} nicht lesbar", p.display()))?
                .resampled_to(rate);
            let bpm = analysis::analyze(&track).bpm;
            (track, bpm)
        }
        None => {
            let (loop_samples, bpm) = match variante {
                Variante::A => (synth::loop_a(rate), 128.0),
                Variante::B => (synth::loop_b(rate), 124.0),
            };
            // Der Loop ist wenige Sekunden lang; ein Deck würde ihn einmal
            // abspielen und dann verstummen. Also so oft aneinanderhängen,
            // dass er die Renderdauer trägt.
            let samples = wiederholen(loop_samples, rate, mindestlaenge);
            (
                Track {
                    samples,
                    sample_rate: rate,
                    stems: Vec::new(),
                },
                Some(bpm),
            )
        }
    };

    let state = Arc::new(DeckState::new());
    state.set_playing(true);
    state.set_keylock(true);

    let voice = Voice::new(Arc::new(track), Arc::clone(&state));
    Ok((Deck { bpm, state }, Box::new(DeckSource::new(voice))))
}

fn wiederholen(samples: Vec<f32>, rate: u32, mindestlaenge: f64) -> Vec<f32> {
    if samples.is_empty() {
        return samples;
    }
    let noetig = (mindestlaenge * rate as f64) as usize * 2;
    let male = (noetig / samples.len()).max(1) + 1;

    let mut out = Vec::with_capacity(samples.len() * male);
    for _ in 0..male {
        out.extend_from_slice(&samples);
    }
    out
}

/// Kleine Klangerzeugung für den Fall ohne Eingabedateien.
mod synth {
    use std::f32::consts::PI;

    pub fn loop_a(rate: u32) -> Vec<f32> {
        bauen(rate, 128.0, 8, |bar, step, t, out| {
            if step % 4 == 0 {
                kick(t, out);
            }
            if step % 4 == 2 {
                hat(t, out, 0.25);
            }
            if step % 2 == 0 {
                let note = [55.0, 55.0, 73.42, 65.41][bar % 4];
                bass(t, note, out, 0.30);
            }
        })
    }

    pub fn loop_b(rate: u32) -> Vec<f32> {
        bauen(rate, 124.0, 8, |bar, step, t, out| {
            if step % 4 == 0 {
                kick(t, out);
            }
            if step % 8 == 4 {
                snare(t, out);
            }
            if step % 2 == 1 {
                let note = [49.0, 61.74, 49.0, 58.27][bar % 4];
                bass(t, note, out, 0.28);
            }
        })
    }

    /// Baut `bars` Takte à 16 Sechzehntel und ruft je Schritt den Aufbau auf.
    fn bauen(
        rate: u32,
        bpm: f32,
        bars: usize,
        mut schritt: impl FnMut(usize, usize, f32, &mut dyn FnMut(usize, f32)),
    ) -> Vec<f32> {
        let frames_per_beat = rate as f32 * 60.0 / bpm;
        let frames_per_step = frames_per_beat / 4.0;
        let total = (frames_per_step * 16.0 * bars as f32) as usize;
        let mut mono = vec![0.0f32; total];

        for bar in 0..bars {
            for step in 0..16 {
                let start = ((bar * 16 + step) as f32 * frames_per_step) as usize;
                let laenge = (frames_per_step * 4.0) as usize;

                let mut add = |offset: usize, value: f32| {
                    let idx = start + offset;
                    if idx < total {
                        mono[idx] += value;
                    }
                };

                schritt(bar, step, laenge as f32 / rate as f32, &mut add);
                let _ = laenge;
            }
        }

        // Auf Stereo aufziehen und Pegel bändigen.
        let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-6);
        let scale = 0.7 / peak;
        mono.into_iter()
            .flat_map(|v| {
                let s = v * scale;
                [s, s]
            })
            .collect()
    }

    fn kick(dauer: f32, add: &mut dyn FnMut(usize, f32)) {
        let n = (dauer * 48_000.0).min(12_000.0) as usize;
        for i in 0..n {
            let t = i as f32 / 48_000.0;
            let env = (-t * 22.0).exp();
            // Tonhöhe fällt — das macht den Punch aus.
            let f = 110.0 * (-t * 30.0).exp() + 45.0;
            add(i, (2.0 * PI * f * t).sin() * env * 0.9);
        }
    }

    fn snare(dauer: f32, add: &mut dyn FnMut(usize, f32)) {
        let n = (dauer * 48_000.0).min(9_000.0) as usize;
        let mut seed = 0x2545_F491u32;
        for i in 0..n {
            let t = i as f32 / 48_000.0;
            let env = (-t * 30.0).exp();
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
            let ton = (2.0 * PI * 190.0 * t).sin();
            add(i, (noise * 0.6 + ton * 0.4) * env * 0.6);
        }
    }

    fn hat(dauer: f32, add: &mut dyn FnMut(usize, f32), gain: f32) {
        let n = (dauer * 48_000.0).min(2_400.0) as usize;
        let mut seed = 0x9E37_79B9u32;
        for i in 0..n {
            let t = i as f32 / 48_000.0;
            let env = (-t * 120.0).exp();
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / 8_388_608.0 - 1.0;
            add(i, noise * env * gain);
        }
    }

    fn bass(dauer: f32, freq: f32, add: &mut dyn FnMut(usize, f32), gain: f32) {
        let n = (dauer * 48_000.0).min(14_000.0) as usize;
        for i in 0..n {
            let t = i as f32 / 48_000.0;
            let env = (1.0 - (-t * 120.0).exp()) * (-t * 6.0).exp();
            let saw = 2.0 * ((freq * t) % 1.0) - 1.0;
            add(i, saw * env * gain);
        }
    }
}

fn schreibe_wav(pfad: &Path, pcm: &[f32], rate: u32) -> std::io::Result<()> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = BufWriter::new(File::create(pfad)?);

    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&(rate * 4).to_le_bytes())?;
    out.write_all(&4u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;

    for sample in pcm {
        let value = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
        out.write_all(&value.to_le_bytes())?;
    }

    out.flush()
}

fn parse_args() -> Result<Options> {
    let mut opts = Options::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        let mut wert = || args.next().context("fehlender Wert");
        match arg.as_str() {
            "--out" | "-o" => opts.out = PathBuf::from(wert()?),
            "--cue" => opts.cue_out = Some(PathBuf::from(wert()?)),
            "--a" => opts.a = Some(PathBuf::from(wert()?)),
            "--b" => opts.b = Some(PathBuf::from(wert()?)),
            "--aux" => opts.aux = Some(PathBuf::from(wert()?)),
            "--seconds" => opts.seconds = wert()?.parse().context("--seconds")?,
            "--transition" => opts.transition = Some(wert()?.parse().context("--transition")?),
            "--transition-len" => {
                opts.transition_len = wert()?.parse().context("--transition-len")?
            }
            "--rate" => opts.sample_rate = wert()?.parse().context("--rate")?,
            "-h" | "--help" => {
                hilfe();
                std::process::exit(0);
            }
            other => bail!("unbekannte Option: {other}"),
        }
    }

    if opts.seconds <= 0.0 {
        bail!("--seconds muss positiv sein");
    }
    Ok(opts)
}

fn hilfe() {
    println!("Aufruf: musik-mix [--a <datei>] [--b <datei>] [--aux <datei>] --out <mix.wav>");
    println!();
    println!("Rendert einen Übergang zwischen zwei Decks offline in eine WAV-Datei.");
    println!("Ohne --a/--b werden synthetische Loops verwendet.");
    println!();
    println!("  --a <datei>            Deck A");
    println!("  --b <datei>            Deck B");
    println!("  --aux <datei>          AUX-Zuspieler, läuft auf Thru durch");
    println!("  --out <datei>          Ausgabe (Vorgabe: mix.wav)");
    println!("  --cue <datei>          Kopfhörer-Bus als eigene Datei; Deck B liegt");
    println!("                         darauf vor, während A auf der Summe läuft");
    println!("  --seconds <n>          Gesamtlänge (Vorgabe: 60)");
    println!("  --transition <sek>     Beginn des Übergangs (Vorgabe: Mitte)");
    println!("  --transition-len <sek> Dauer des Übergangs (Vorgabe: 16)");
    println!("  --rate <hz>            Samplerate (Vorgabe: 48000)");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summe_und_kopfhoerer_kommen_aus_den_richtigen_kanaelen() {
        // Zwei Frames: erst die Summe, dann der Kopfhörer.
        let block = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut summe = Vec::new();
        let mut kopfhoerer = Vec::new();
        aufteilen(&block, 4, &mut summe, &mut kopfhoerer);

        assert_eq!(summe, vec![1.0, 2.0, 5.0, 6.0]);
        assert_eq!(kopfhoerer, vec![3.0, 4.0, 7.0, 8.0]);
    }
}
