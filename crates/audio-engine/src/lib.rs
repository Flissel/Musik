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

pub mod aufnahme;
pub mod aux;
pub mod channel;
pub mod command;
pub mod crossfader;
pub mod effects;
pub mod eingang;
pub mod eq;
pub mod filter;
pub mod limiter;
pub mod mixer;
pub mod output;
pub mod source;
pub mod svf;
pub mod sync;

#[cfg(test)]
pub(crate) mod testing;

pub use aufnahme::{mitschnitt, Aufnahme, Mitschnitt};
pub use aux::{aux_channel, AuxSource, AuxWriter};
pub use channel::Channel;
pub use command::{channel as engine_channel, Command, EngineHandle, EngineRunner};
pub use crossfader::{Assign, Crossfader};
pub use effects::{Effekt, FxUnit};
pub use eingang::Eingang;
pub use eq::ThreeBandEq;
pub use filter::DjFilter;
pub use limiter::Limiter;
pub use mixer::{assign_deck_pair, Engine};
pub use output::Output;
pub use source::{DeckSource, LoopSource, SilentSource, Source};
pub use sync::{phase_error, sync, sync_tempo_only, SyncPlan};
