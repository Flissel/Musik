use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Datei konnte nicht gelesen werden: {0}")]
    Io(#[from] std::io::Error),

    #[error("Dekodierung fehlgeschlagen: {0}")]
    Decode(#[from] symphonia::core::errors::Error),

    #[error("keine Audiospur in der Datei gefunden")]
    NoAudioTrack,

    #[error("kein Ausgabegerät gefunden")]
    NoOutputDevice,

    #[error("Audio-Gerät: {0}")]
    Device(#[from] cpal::Error),

    #[error("Sample-Format {0} wird nicht unterstützt")]
    UnsupportedSampleFormat(String),
}

pub type Result<T> = std::result::Result<T, AudioError>;
