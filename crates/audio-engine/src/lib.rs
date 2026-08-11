//! Mixer-Engine: mehrere Kanäle, Master- und Cue-Bus.
//!
//! Setzt auf `audio-core` auf, das ein einzelnes Deck liefert. Hier kommt
//! zusammen, was daraus ein Mischpult macht — Kanalzüge mit EQ und Filter,
//! Crossfader, Begrenzer auf der Summe und ein getrennter Kopfhörerweg.
//!
//! Quellen sind austauschbar: Ein Kanal weiß nicht, ob hinter ihm ein Deck,
//! ein AUX-Eingang oder später ein generierter Track steht.
//!
//! Alles hier läuft im Audio-Callback und hält sich an dessen Regeln — keine
//! Allokation, kein Lock, keine Speicherfreigabe. Puffer wachsen beim ersten
//! Block auf die nötige Größe und bleiben danach stehen.

pub mod aux;
pub mod channel;
pub mod crossfader;
pub mod eq;
pub mod filter;
pub mod limiter;
pub mod mixer;
pub mod source;
pub mod svf;

#[cfg(test)]
pub(crate) mod testing;

pub use aux::{aux_channel, AuxSource, AuxWriter};
pub use channel::Channel;
pub use crossfader::{Assign, Crossfader};
pub use eq::ThreeBandEq;
pub use filter::DjFilter;
pub use limiter::Limiter;
pub use mixer::{assign_deck_pair, Engine};
pub use source::{DeckSource, LoopSource, SilentSource, Source};
