//! Der Zuspieler für den AUX-Kanal: externes Audio von einem Gerät.
//!
//! Der AUX-Kanal gab es lange, aber er war leer. Im Mixer steht ein
//! vollständiger Kanalzug — Trim, EQ, Filter, Fader, Cue —, und in den
//! Ringpuffer davor hat nie jemand geschrieben. Im Programm stand das sogar
//! wörtlich: „AUX bleibt ohne Zuspieler still."
//!
//! Hier ist der Zuspieler. Er öffnet ein Aufnahmegerät und schiebt, was
//! hereinkommt, in den Ring — das Gegenstück zu [`crate::output`], und aus
//! demselben Grund ein eigener Callback: Aufnahme und Wiedergabe sind zwei
//! Uhren, und der Ring dazwischen ist das, was sie entkoppelt.
//!
//! # Zwei Dinge müssen stimmen, sonst wird es still gefälscht
//!
//! **Die Samplerate.** Der Ring trägt Samples, keine Zeit. Nimmt das Gerät mit
//! 44,1 kHz auf und rechnet die Engine mit 48, läuft alles um 8 % zu langsam —
//! und nichts sagt es, denn es klingt wie Musik. Deshalb wird eine
//! Konfiguration mit **genau** der Rate der Engine verlangt und sonst
//! abgelehnt. Umrechnen wäre ein eigener Baustein; falsch abspielen ist keiner.
//!
//! **Die Kanalzahl.** Der Kanal erwartet verschränktes Stereo. Ein Mikrofon
//! liefert mono, ein Interface womöglich acht Kanäle. Beides wird auf zwei
//! gebracht — mono verdoppelt, mehr als zwei auf die ersten beiden gekürzt.
//!
//! # Ungeprüft an echter Hardware
//!
//! Hier gibt es kein Audiogerät. Geprüft ist, was sich ohne eines prüfen lässt:
//! die Umrechnung auf Stereo und dass die Fehlermeldungen greifen. Ob der
//! Treiber tut, was seine Beschreibung sagt, weiß erst, wer es anschließt —
//! dieselbe Lücke wie bei den vier Ausgängen.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};

use crate::aux::AuxWriter;

/// Ein offenes Aufnahmegerät, das in den AUX-Ring schreibt.
pub struct Eingang {
    device_name: String,
    sample_rate: u32,
    channels: usize,
    _stream: cpal::Stream,
}

impl Eingang {
    /// Öffnet ein Aufnahmegerät und übergibt ihm die schreibende Seite.
    ///
    /// `wunsch` wählt das Gerät über einen Teil seines Namens; ohne Angabe das
    /// Standardgerät. `rate` ist die Rate, mit der die Engine rechnet — eine
    /// andere wird abgelehnt, nicht umgerechnet.
    pub fn open(writer: AuxWriter, rate: u32, wunsch: Option<&str>) -> Result<Eingang, String> {
        let host = cpal::default_host();
        let device = match wunsch {
            Some(name) => geraet_suchen(&host, name)?,
            None => host
                .default_input_device()
                .ok_or_else(|| "kein Aufnahmegerät gefunden".to_string())?,
        };
        let device_name = name_von(&device);

        let config = passende_konfiguration(&device, rate)?;
        let format = config.1;
        let config: StreamConfig = config.0;
        let channels = config.channels as usize;

        let stream = match format {
            SampleFormat::F32 => bauen::<f32>(&device, config, writer, channels),
            SampleFormat::F64 => bauen::<f64>(&device, config, writer, channels),
            SampleFormat::I32 => bauen::<i32>(&device, config, writer, channels),
            SampleFormat::U32 => bauen::<u32>(&device, config, writer, channels),
            SampleFormat::I16 => bauen::<i16>(&device, config, writer, channels),
            SampleFormat::U16 => bauen::<u16>(&device, config, writer, channels),
            // Acht Bit ist die letzte Wahl und nie die erste — aber besser als
            // ein Gerät, das sich nicht öffnen lässt.
            SampleFormat::I8 => bauen::<i8>(&device, config, writer, channels),
            SampleFormat::U8 => bauen::<u8>(&device, config, writer, channels),
            other => return Err(format!("Sample-Format {other} nicht unterstützt")),
        }
        .map_err(|e| format!("Aufnahme-Stream ließ sich nicht öffnen: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("Aufnahme ließ sich nicht starten: {e}"))?;

        Ok(Eingang {
            device_name,
            sample_rate: rate,
            channels,
            _stream: stream,
        })
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Wie viele Kanäle das Gerät liefert — vor der Umrechnung auf Stereo.
    pub fn channels(&self) -> usize {
        self.channels
    }
}

/// Alle Aufnahmegeräte, die der Host kennt.
pub fn geraete() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(liste) => liste.map(|d| name_von(&d)).collect(),
        Err(_) => Vec::new(),
    }
}

fn name_von(device: &cpal::Device) -> String {
    device
        .id()
        .map(|id| id.to_string())
        .unwrap_or_else(|_| "unbekannt".into())
}

fn geraet_suchen(host: &cpal::Host, wunsch: &str) -> Result<cpal::Device, String> {
    let liste = host
        .input_devices()
        .map_err(|e| format!("Aufnahmegeräte nicht auflistbar: {e}"))?;
    let klein = wunsch.to_lowercase();
    for device in liste {
        if name_von(&device).to_lowercase().contains(&klein) {
            return Ok(device);
        }
    }
    Err(format!(
        "kein Aufnahmegerät, dessen Name „{wunsch}\" enthält — bekannt sind: {}",
        geraete().join(", ")
    ))
}

/// Sucht eine Konfiguration mit **genau** dieser Rate und dem besten Format.
///
/// Nicht die nächstbeste Rate: Eine Rate daneben heißt ein Tempo daneben, und
/// das fällt niemandem auf, weil es wie Musik klingt.
///
/// Und nicht das erstbeste Format. Am `null`-Gerät von ALSA stand `i8` an
/// erster Stelle — acht Bit, also rund 48 dB Störabstand für Material, das
/// hinterher analysiert werden soll. Wer die Liste der Reihe nach abarbeitet,
/// nimmt das und merkt nichts.
fn passende_konfiguration(
    device: &cpal::Device,
    rate: u32,
) -> Result<(StreamConfig, SampleFormat), String> {
    let moeglich = device
        .supported_input_configs()
        .map_err(|e| format!("Gerätekonfigurationen nicht lesbar: {e}"))?;

    let mut gesehen = Vec::new();
    let mut beste: Option<(u8, StreamConfig, SampleFormat)> = None;
    for bereich in moeglich {
        let von = bereich.min_sample_rate();
        let bis = bereich.max_sample_rate();
        gesehen.push(format!("{von}–{bis}"));
        if von > rate || rate > bis {
            continue;
        }
        let format = bereich.sample_format();
        let rang = rang(format);
        if beste.as_ref().is_none_or(|(bisher, ..)| rang > *bisher) {
            beste = Some((rang, bereich.with_sample_rate(rate).into(), format));
        }
    }

    match beste {
        Some((_, config, format)) => Ok((config, format)),
        None => Err(format!(
            "{} kann nicht mit {rate} Hz aufnehmen (es kann: {}). Umgerechnet wird \
             nicht — eine falsche Rate klingt wie Musik und ist keine.",
            name_von(device),
            gesehen.join(", ")
        )),
    }
}

/// Wie brauchbar ein Sample-Format für Material ist, das analysiert wird.
///
/// Höher ist besser. Entscheidend ist die Auflösung: Acht Bit sind rund 48 dB
/// Störabstand, sechzehn schon 96. Gleitkomma steht oben, weil es das ist,
/// womit gerechnet wird — jede Umrechnung mehr ist eine Rundung mehr.
fn rang(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 100,
        SampleFormat::F64 => 90,
        SampleFormat::I32 | SampleFormat::U32 => 80,
        SampleFormat::I16 | SampleFormat::U16 => 70,
        SampleFormat::I8 | SampleFormat::U8 => 10,
        _ => 50,
    }
}

/// Bringt einen Block auf verschränktes Stereo.
///
/// Mono wird verdoppelt, mehr als zwei Kanäle auf die ersten beiden gekürzt.
/// Ein unvollständiger letzter Frame wird verworfen — halbe Frames bringen die
/// Kanäle für den Rest der Aufnahme durcheinander.
fn nach_stereo<T>(data: &[T], kanaele: usize, aus: &mut Vec<f32>)
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    aus.clear();
    if kanaele == 0 {
        return;
    }
    if kanaele == 1 {
        for s in data {
            let v = s.to_sample::<f32>();
            aus.push(v);
            aus.push(v);
        }
        return;
    }
    for frame in data.chunks_exact(kanaele) {
        aus.push(frame[0].to_sample::<f32>());
        aus.push(frame[1].to_sample::<f32>());
    }
}

fn bauen<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut writer: AuxWriter,
    kanaele: usize,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    // Einmal groß genug angelegt und danach nur noch gefüllt: Im Callback wird
    // nichts angelegt, genau wie überall sonst im Audiopfad.
    let mut block: Vec<f32> = Vec::with_capacity(8_192);

    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            nach_stereo(data, kanaele, &mut block);
            writer.write(&block);
        },
        |err| eprintln!("Aufnahme-Stream: {err}"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Gleitkomma schlägt acht Bit.**
    ///
    /// Am `null`-Gerät von ALSA steht `i8` an erster Stelle der Liste. Wer sie
    /// der Reihe nach abarbeitet, nimmt acht Bit für Material, das hinterher
    /// analysiert werden soll — und merkt nichts, denn es funktioniert ja.
    #[test]
    fn ein_gutes_format_schlaegt_ein_frueheres_schlechtes() {
        assert!(rang(SampleFormat::F32) > rang(SampleFormat::I8));
        assert!(rang(SampleFormat::I16) > rang(SampleFormat::U8));
        assert!(rang(SampleFormat::I32) > rang(SampleFormat::I16));
    }

    #[test]
    fn mono_wird_verdoppelt() {
        let mut aus = Vec::new();
        nach_stereo(&[0.5f32, -0.25], 1, &mut aus);
        assert_eq!(aus, vec![0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn stereo_bleibt_stereo() {
        let mut aus = Vec::new();
        nach_stereo(&[0.5f32, -0.5, 0.25, -0.25], 2, &mut aus);
        assert_eq!(aus, vec![0.5, -0.5, 0.25, -0.25]);
    }

    /// **Ein Interface mit acht Eingängen darf nicht acht Kanäle durchreichen.**
    /// Der AUX-Kanal ist stereo; alles darüber gehört abgeschnitten, nicht
    /// hineingemischt.
    #[test]
    fn mehr_als_zwei_kanaele_werden_gekuerzt() {
        let mut aus = Vec::new();
        let block: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        nach_stereo(&block, 4, &mut aus);
        assert_eq!(aus, vec![1.0, 2.0, 5.0, 6.0]);
    }

    /// **Ein halber Frame am Blockende wird verworfen, nicht geraten.**
    ///
    /// Wer ihn mitnimmt, verschiebt links und rechts gegeneinander — und zwar
    /// nicht für einen Block, sondern für den Rest der Aufnahme.
    #[test]
    fn ein_halber_frame_am_ende_wird_verworfen() {
        let mut aus = Vec::new();
        nach_stereo(&[1.0f32, 2.0, 3.0], 2, &mut aus);
        assert_eq!(aus, vec![1.0, 2.0]);
    }

    #[test]
    fn null_kanaele_ergeben_nichts_statt_einer_panik() {
        let mut aus = Vec::new();
        nach_stereo(&[1.0f32, 2.0], 0, &mut aus);
        assert!(aus.is_empty());
    }

    /// Der Puffer wird wiederverwendet — Reste des letzten Blocks dürfen nicht
    /// stehen bleiben.
    #[test]
    fn der_puffer_wird_vor_jedem_block_geleert() {
        let mut aus = Vec::new();
        nach_stereo(&[1.0f32, 2.0, 3.0, 4.0], 2, &mut aus);
        nach_stereo(&[9.0f32, 8.0], 2, &mut aus);
        assert_eq!(aus, vec![9.0, 8.0]);
    }
}
