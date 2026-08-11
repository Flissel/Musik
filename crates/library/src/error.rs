use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("Datenbankfehler: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("Datei konnte nicht gelesen werden: {0}")]
    Io(#[from] std::io::Error),

    #[error("XML konnte nicht gelesen werden: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("nicht gefunden: {0}")]
    NotFound(String),

    #[error("unerwartetes Format: {0}")]
    Format(String),
}

pub type Result<T> = std::result::Result<T, LibraryError>;
