//! Anbindung an das Ausgabegerät via CPAL.
//!
//! Der Track wird beim Öffnen einmalig auf die Geräte-Samplerate gebracht.
//! Dadurch muss der Callback nur noch Tempo behandeln und keine
//! Ratenkonvertierung — eine Rechenstufe weniger im zeitkritischen Pfad.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::deck::{DeckState, Voice};
use crate::error::{AudioError, Result};
use crate::track::Track;

pub struct Player {
    state: Arc<DeckState>,
    sample_rate: u32,
    device_name: String,
    // Das Abreißen des Streams stoppt die Wiedergabe — er muss am Leben bleiben.
    _stream: cpal::Stream,
}

impl Player {
    /// Öffnet das Standard-Ausgabegerät und legt den Track auf ein Deck.
    pub fn open(track: Track) -> Result<Player> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;
        let device_name = device
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| "unbekannt".to_string());

        let supported = device.default_output_config()?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let sample_rate = config.sample_rate;

        let track = Arc::new(track.resampled_to(sample_rate));
        let state = Arc::new(DeckState::new());
        let voice = Voice::new(Arc::clone(&track), Arc::clone(&state));

        let stream = match sample_format {
            SampleFormat::F32 => build::<f32>(&device, config, voice)?,
            SampleFormat::I16 => build::<i16>(&device, config, voice)?,
            SampleFormat::I32 => build::<i32>(&device, config, voice)?,
            SampleFormat::U16 => build::<u16>(&device, config, voice)?,
            SampleFormat::U8 => build::<u8>(&device, config, voice)?,
            SampleFormat::F64 => build::<f64>(&device, config, voice)?,
            other => {
                return Err(AudioError::UnsupportedSampleFormat(other.to_string()));
            }
        };

        stream.play()?;

        Ok(Player {
            state,
            sample_rate,
            device_name,
            _stream: stream,
        })
    }

    pub fn state(&self) -> &Arc<DeckState> {
        &self.state
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn position_secs(&self) -> f64 {
        self.state.position_frames() as f64 / self.sample_rate as f64
    }

    pub fn seek_secs(&self, secs: f64) {
        let frame = (secs.max(0.0) * self.sample_rate as f64) as u64;
        self.state.seek_frames(frame);
    }
}

fn build<T>(device: &cpal::Device, config: StreamConfig, mut voice: Voice) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    // Zwischenpuffer in f32; der Callback allokiert dadurch nicht.
    let mut block: Vec<f32> = Vec::new();

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            if block.len() < data.len() {
                block.resize(data.len(), 0.0);
            }
            let scratch = &mut block[..data.len()];
            voice.render(scratch, channels);

            for (dst, src) in data.iter_mut().zip(scratch.iter()) {
                *dst = T::from_sample(*src);
            }
        },
        |err| eprintln!("Audio-Stream: {err}"),
        None,
    )?;

    Ok(stream)
}
