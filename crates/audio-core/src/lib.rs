//! Audio-Kern für ein Traktor-artiges DJ-Tool.
//!
//! Phase 0: ein Deck, Wiedergabe, Tempo mit und ohne Keylock. Der Zweck ist,
//! die Stack-Entscheidung nativ vs. Browser empirisch zu belegen — siehe
//! `docs/BAUSTEINE.md`.

pub mod deck;
pub mod error;
pub mod grid;
pub mod player;
pub mod stretch;
pub mod struktur;
pub mod tonart;
pub mod track;

#[cfg(test)]
mod testing;

pub use deck::DeckState;
pub use error::{AudioError, Result};
pub use grid::Beatgrid;
pub use player::Player;
pub use struktur::{Abschnitt, Art, PHRASE_BEATS, Struktur};
pub use tonart::Tonart;
pub use track::{MAX_STEMS, Stem, Track};
