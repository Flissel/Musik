//! Dekodieren einer Audiodatei in einen zusammenhängenden Stereo-Puffer.
//!
//! Bewusst „alles in den Speicher": Ein Deck darf im Audio-Callback nicht auf
//! die Platte warten, und ein 5-Minuten-Track kostet als f32-Stereo rund 100 MB
//! — vertretbar für Phase 0. Streaming von Platte kommt, wenn die Library steht.

use std::fs::File;
use std::path::Path;

use symphonia::core::formats::TrackType;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;

use crate::error::{AudioError, Result};

/// Ein vollständig dekodierter Track als interleaved Stereo (L, R, L, R, …).
pub struct Track {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub const CHANNELS: usize = 2;

impl Track {
    pub fn frames(&self) -> usize {
        self.samples.len() / CHANNELS
    }

    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// Dekodiert eine Datei. Format wird über Inhalt und Dateiendung erkannt.
    pub fn decode_file(path: &Path) -> Result<Track> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let mut format = symphonia::default::get_probe().probe(
            &hint,
            mss,
            Default::default(),
            Default::default(),
        )?;

        let track = format
            .default_track(TrackType::Audio)
            .ok_or(AudioError::NoAudioTrack)?;
        let track_id = track.id;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or(AudioError::NoAudioTrack)?;

        let mut decoder =
            symphonia::default::get_codecs().make_audio_decoder(params, &Default::default())?;

        let mut samples: Vec<f32> = Vec::new();
        let mut scratch: Vec<f32> = Vec::new();
        let mut sample_rate = 0u32;

        while let Some(packet) = format.next_packet()? {
            if packet.track_id != track_id {
                continue;
            }

            let decoded = match decoder.decode(&packet) {
                Ok(d) => d,
                // Einzelne defekte Pakete überspringen statt den ganzen Track
                // zu verlieren — kommt bei geschnittenen MP3s regelmäßig vor.
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => return Err(e.into()),
            };

            let frames = decoded.frames();
            if frames == 0 {
                continue;
            }
            sample_rate = decoded.spec().rate();
            let src_channels = decoded.samples_interleaved() / frames;

            decoded.copy_to_vec_interleaved(&mut scratch);
            append_as_stereo(&mut samples, &scratch, src_channels, frames);
        }

        if samples.is_empty() || sample_rate == 0 {
            return Err(AudioError::NoAudioTrack);
        }

        Ok(Track {
            samples,
            sample_rate,
        })
    }

    /// Bringt den Track auf die Rate des Ausgabegeräts.
    ///
    /// Läuft einmalig beim Laden, nicht im Audio-Callback — deshalb ist lineare
    /// Interpolation hier unkritisch. Wandert das je in den Abspielpfad, gehört
    /// ein richtiger Resampler her (siehe docs/BAUSTEINE.md).
    pub fn resampled_to(self, target_rate: u32) -> Track {
        if target_rate == self.sample_rate {
            return self;
        }

        let ratio = self.sample_rate as f64 / target_rate as f64;
        let src_frames = self.frames();
        let dst_frames = ((src_frames as f64) / ratio).floor() as usize;
        let mut out = vec![0.0f32; dst_frames * CHANNELS];

        for i in 0..dst_frames {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let next = (idx + 1).min(src_frames - 1);
            for c in 0..CHANNELS {
                let a = self.samples[idx * CHANNELS + c];
                let b = self.samples[next * CHANNELS + c];
                out[i * CHANNELS + c] = a + (b - a) * frac;
            }
        }

        Track {
            samples: out,
            sample_rate: target_rate,
        }
    }
}

/// Hängt einen dekodierten Block als Stereo an. Mono wird dupliziert, bei mehr
/// als zwei Kanälen werden die ersten beiden genommen.
fn append_as_stereo(dst: &mut Vec<f32>, src: &[f32], src_channels: usize, frames: usize) {
    match src_channels {
        0 => {}
        1 => {
            for &s in &src[..frames] {
                dst.push(s);
                dst.push(s);
            }
        }
        n => {
            for i in 0..frames {
                dst.push(src[i * n]);
                dst.push(src[i * n + 1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::dominant_freq;
    use std::path::PathBuf;

    const RATE: u32 = 44_100;

    fn wav_pcm16(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::with_capacity(44 + data_len as usize);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    /// Schreibt eine Mono-WAV mit 1 kHz Sinus und gibt den Pfad zurück.
    fn temp_wav(name: &str, secs: f32) -> PathBuf {
        let frames = (RATE as f32 * secs) as usize;
        let step = 2.0 * std::f32::consts::PI * 1000.0 / RATE as f32;
        let pcm: Vec<i16> = (0..frames)
            .map(|i| ((step * i as f32).sin() * 20_000.0) as i16)
            .collect();

        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, wav_pcm16(&pcm, 1, RATE)).expect("WAV schreiben");
        path
    }

    #[test]
    fn mono_wird_zu_stereo_dekodiert() {
        let path = temp_wav("audio-core-mono.wav", 0.5);
        let track = Track::decode_file(&path).expect("dekodieren");
        std::fs::remove_file(&path).ok();

        assert_eq!(track.sample_rate, RATE);
        assert_eq!(track.samples.len(), track.frames() * CHANNELS);
        assert!(
            (track.frames() as i64 - (RATE as f32 * 0.5) as i64).abs() < 64,
            "unerwartete Länge: {} Frames",
            track.frames()
        );

        // Mono muss auf beide Kanäle gelegt worden sein.
        for i in (0..track.samples.len()).step_by(CHANNELS) {
            assert_eq!(track.samples[i], track.samples[i + 1]);
        }
    }

    #[test]
    fn resampling_haelt_die_tonhoehe() {
        let path = temp_wav("audio-core-resample.wav", 1.0);
        let track = Track::decode_file(&path).expect("dekodieren");
        std::fs::remove_file(&path).ok();

        let resampled = track.resampled_to(48_000);

        assert_eq!(resampled.sample_rate, 48_000);
        assert!(
            (resampled.frames() as i64 - 48_000).abs() < 128,
            "unerwartete Länge nach Resampling: {} Frames",
            resampled.frames()
        );

        let freq = dominant_freq(&resampled.samples, 48_000);
        assert!(
            (freq - 1000.0).abs() < 10.0,
            "Tonhöhe verschoben: {freq:.1} Hz statt 1000 Hz"
        );
    }

    #[test]
    fn resampling_auf_gleiche_rate_ist_ein_no_op() {
        let path = temp_wav("audio-core-noop.wav", 0.2);
        let track = Track::decode_file(&path).expect("dekodieren");
        std::fs::remove_file(&path).ok();

        let frames_vorher = track.frames();
        let gleich = track.resampled_to(RATE);

        assert_eq!(gleich.frames(), frames_vorher);
    }
}
