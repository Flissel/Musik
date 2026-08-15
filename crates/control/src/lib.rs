//! Ein benannter Steuerraum für alles, was bedienbar ist.
//!
//! Der Gedanke stammt von Mixxx: Dort laufen Tastatur, MIDI, HID und die
//! Oberfläche über *dieselben* benannten Controls, statt dass jede Bedienart
//! ihren eigenen Draht zum Mixer zieht. Das ist die richtige Idee, und sie ist
//! frei — geschützt ist die Ausdrucksform, nicht der Entwurf (siehe
//! `docs/MIXXX.md`).
//!
//! Drei Dinge machen wir anders, und sie sind der Grund, warum sich der Bau
//! lohnt:
//!
//! 1. **Der Steuerraum beschreibt sich selbst.** Jedes Control trägt Bereich,
//!    Einheit, Schreibbarkeit und einen Satz zur Bedeutung. Bei Mixxx steht das
//!    im Handbuch; ein laufendes Programm lässt sich nicht fragen, was es kann.
//!    Ein Agent, der hier `list` sagt, weiß danach genug, um zu bedienen.
//! 2. **Werte sind typisiert.** Ein Schalter ist ein Schalter, eine Auswahl
//!    trägt ihre Namen. Nicht alles ist ein `double`, in das jeder Tippfehler
//!    als gültiger Wert hineinpasst.
//! 3. **Er ist von außen erreichbar.** Mixxx spricht OSC nur ausgehend;
//!    Befehle entgegenzunehmen ist dort ein seit Jahren offener Wunsch. Hier
//!    ist es der Zweck.
//! 4. **Er kennt Zeit.** Ein Übergang ist eine Bewegung über Takte, keine
//!    Folge von Reglerstellungen. Wer „Bass raus über acht Beats" sagen kann,
//!    muss nicht in einer engen Schleife pollen — siehe [`zeitplan`].
//!
//! Echtzeit bleibt unangetastet: Das Pult schreibt nie in den Audio-Callback,
//! sondern schickt Kommandos in dieselbe lock-freie Schlange, die die
//! Oberfläche benutzt. Gelesen wird aus Atomics und aus dem Spiegel, nie aus
//! dem Mixer selbst.

pub mod katalog;
pub mod protokoll;
pub mod pult;
pub mod schluessel;
pub mod server;
pub mod warteschlange;
pub mod wert;
pub mod zeitplan;

#[cfg(test)]
pub(crate) mod testing;

pub use katalog::Beschreibung;
pub use protokoll::behandle;
pub use protokoll::Sitzung;
pub use pult::{
    assign_aus_name, assign_name, DeckEintrag, Fehler, KanalSpiegel, MasterSpiegel, Sammlung,
    Steuerpult, Treffer,
};
pub use schluessel::{Gruppe, Schluessel};
pub use server::{Server, ServerFehler};
pub use warteschlange::Warteschlange;
pub use wert::{Art, Einheit, Wert};
pub use zeitplan::{Zeitplan, TAKT};
