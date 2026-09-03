//! Die Sammlung: Tracks, Marker, Playlists — und der Weg herein aus Traktor.
//!
//! Getrennt von Engine und Analyse, weil hier keine Echtzeitregeln gelten.
//! Datenbankzugriffe dürfen blockieren; im Audio-Callback hat davon nichts
//! etwas zu suchen.
//!
//! Zwei Entwurfsentscheidungen prägen alles Weitere:
//!
//! **Positionen stehen in Millisekunden**, nicht in Frames. Die Library ist die
//! Tauschschicht zwischen Analyse, Traktor-Import und Deck. Ein Frame-Wert ohne
//! zugehörige Samplerate ist mehrdeutig, und ein resampelter Track hätte andere
//! Zahlen für denselben Zeitpunkt.
//!
//! **Lizenz und Urheber stehen von der ersten Migration an in der Tabelle.**
//! CC BY verlangt Namensnennung auch nicht-kommerziell, und die Herkunft von
//! tausend Samples lässt sich nachträglich nicht rekonstruieren.

pub mod db;
pub mod error;
pub mod model;
pub mod traktor;

pub use db::Library;
pub use error::{LibraryError, Result};
pub use model::{CueKind, CueRecord, Query, Source, TrackRecord};
pub use traktor::{import_nml, ImportReport};
