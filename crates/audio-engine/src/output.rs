//! Ausgabe der Engine auf ein Audiogerät.
//!
//! Sucht eine Gerätekonfiguration mit vier Ausgängen, damit der Cue-Bus auf
//! 3/4 landet. Gibt es die nicht, läuft die Software trotzdem — dann eben ohne
//! Vorhören, und sie sagt das auch.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::command::EngineRunner;
use crate::mixer::Engine;

/// Fehlschlag beim Öffnen — **mit** dem Runner zurück.
///
/// Ohne ihn wäre der Mixer verloren, sobald kein Gerät da ist. Der Aufrufer
/// soll ihn weiterverwenden können, etwa für einen Leerlauf-Taktgeber.
///
/// Wird als `Box` zurückgegeben: der Runner macht den Fehlerwert groß, und
/// jeder Erfolgsfall müsste ihn sonst mitschleppen.
pub struct OpenError {
    pub message: String,
    pub runner: EngineRunner,
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub struct Output {
    device_name: String,
    sample_rate: u32,
    channels: usize,
    _stream: cpal::Stream,
}

impl Output {
    /// Öffnet das Standardgerät und übergibt ihm den Mixer.
    pub fn open(runner: EngineRunner) -> Result<Output, Box<OpenError>> {
        macro_rules! fehler {
            ($runner:expr, $($arg:tt)*) => {
                return Err(Box::new(OpenError { message: format!($($arg)*), runner: $runner }))
            };
        }

        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else {
            fehler!(runner, "kein Ausgabegerät gefunden");
        };
        let device_name = device
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| "unbekannt".into());

        let supported = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => fehler!(runner, "Gerätekonfiguration nicht lesbar: {e}"),
        };
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        let sample_rate = config.sample_rate;
        let channels = config.channels as usize;

        // Der Runner wandert in den Callback; scheitert das Öffnen, kommt er
        // von dort zurück.
        let gebaut = match sample_format {
            SampleFormat::F32 => build::<f32>(&device, config, runner),
            SampleFormat::I16 => build::<i16>(&device, config, runner),
            SampleFormat::I32 => build::<i32>(&device, config, runner),
            SampleFormat::U16 => build::<u16>(&device, config, runner),
            SampleFormat::U8 => build::<u8>(&device, config, runner),
            SampleFormat::F64 => build::<f64>(&device, config, runner),
            other => fehler!(runner, "Sample-Format {other} nicht unterstützt"),
        };

        let stream = match gebaut {
            Ok(s) => s,
            Err(boxed) => {
                let (e, runner) = *boxed;
                fehler!(runner, "Stream ließ sich nicht öffnen: {e}")
            }
        };

        if let Err(e) = stream.play() {
            // Hier ist der Runner im Stream gefangen; der Stream wird verworfen.
            return Err(Box::new(OpenError {
                message: format!("Wiedergabe ließ sich nicht starten: {e}"),
                runner: EngineRunner::leer(),
            }));
        }

        Ok(Output {
            device_name,
            sample_rate,
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

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Ob der Cue-Bus einen eigenen Ausgang hat.
    pub fn has_cue_output(&self) -> bool {
        self.channels >= Engine::REQUIRED_OUTPUTS_FOR_CUE
    }
}

#[allow(clippy::type_complexity)]
fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    runner: EngineRunner,
) -> Result<cpal::Stream, Box<(cpal::Error, EngineRunner)>>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut block: Vec<f32> = Vec::new();

    // Der Runner muss im Fehlerfall wieder heraus, also über eine Zelle, die
    // der Callback leert und die wir sonst zurücknehmen.
    let geteilt = std::sync::Arc::new(std::sync::Mutex::new(Some(runner)));
    let fuer_callback = std::sync::Arc::clone(&geteilt);
    let mut eigener: Option<EngineRunner> = None;

    let ergebnis = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            // Einmalig übernehmen, danach nie wieder das Lock anfassen.
            if eigener.is_none() {
                eigener = fuer_callback.lock().ok().and_then(|mut g| g.take());
            }
            let Some(runner) = eigener.as_mut() else {
                data.fill(T::from_sample(0.0));
                return;
            };

            if block.len() < data.len() {
                block.resize(data.len(), 0.0);
            }
            let scratch = &mut block[..data.len()];
            runner.render(scratch, channels);

            for (dst, src) in data.iter_mut().zip(scratch.iter()) {
                *dst = T::from_sample(*src);
            }
        },
        |err| eprintln!("Audio-Stream: {err}"),
        None,
    );

    match ergebnis {
        Ok(stream) => Ok(stream),
        Err(e) => {
            let runner = geteilt
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_else(EngineRunner::leer);
            Err(Box::new((e, runner)))
        }
    }
}
