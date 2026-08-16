//! Das Steuerpult: ein benannter Zugriff auf alles, was bedienbar ist.
//!
//! Bisher lag die Bedienung an drei Stellen — Transport in den Atomics des
//! `DeckState`, Mixer in der Kommandoschlange, und die gespiegelten Reglerwerte
//! in der Oberfläche. Wer von außen steuern wollte, hätte alle drei anfassen
//! müssen, und die Oberfläche war die einzige Stelle, die wusste, wo ein Fader
//! gerade steht.
//!
//! Das Pult dreht das um: Es **besitzt** den Spiegel, schickt jede Änderung
//! selbst in die Schlange und lässt sich unter einem Namen ansprechen. Die
//! Oberfläche wird damit zu einem von mehreren Bedienern — gleichberechtigt mit
//! einem MIDI-Controller, einem Skript oder einem Agenten.
//!
//! Der Mixer selbst wird nie direkt gelesen: Er lebt im Audio-Callback, und
//! dort etwas zu lesen hieße, ein Lock zu nehmen. Stattdessen gilt der
//! Spiegelwert als Wahrheit — er ist es auch, denn niemand sonst schreibt.

use std::sync::Arc;

use audio_core::deck::DeckState;
use audio_core::Beatgrid;
use audio_core::Tonart;
use audio_engine::{Assign, Aufnahme, Command, Effekt, EngineHandle};

use crate::katalog::{self, Beschreibung};
use crate::schluessel::{Gruppe, Schluessel};
use crate::wert::{Art, Wert};

/// Ein Treffer aus der Sammlung, so knapp wie er über die Leitung geht.
#[derive(Debug, Clone, PartialEq)]
pub struct Treffer {
    pub pfad: String,
    pub titel: String,
    pub artist: Option<String>,
    pub bpm: Option<f32>,
    /// Tonart, wie sie in der Sammlung steht — `None`, wenn keine bekannt ist.
    pub tonart: Option<Tonart>,
}

/// Was das Pult nicht selbst kann: suchen und laden.
///
/// Beides braucht Dinge, die hier nichts zu suchen haben — eine SQLite-Datei
/// und einen Dekodierer. Statt `control` von beidem abhängig zu machen,
/// reicht der Betreiber sie herein.
///
/// `laden` **darf nicht blockieren.** Dekodieren und Analysieren dauern
/// Sekunden, und das Pult liegt währenddessen unter einem Mutex, an dem die
/// Oberfläche hängt. Die Umsetzung nimmt den Auftrag an und arbeitet
/// woanders; der Fortschritt kommt über `deckN.load_status` zurück.
pub trait Sammlung: Send {
    fn suchen(&self, text: &str, grenze: usize) -> Vec<Treffer>;
    /// Tracks, die tempomäßig zu `bpm` passen.
    fn suchen_mischbar(&self, bpm: f32, grenze: usize) -> Vec<Treffer>;
    /// Tracks, deren Tonart harmonisch zu `tonart` passt.
    fn suchen_harmonisch(&self, tonart: Tonart, grenze: usize) -> Vec<Treffer>;
    /// Namen aller Playlists.
    fn playlists(&self) -> Vec<String>;
    /// Die Tracks einer Playlist, in ihrer Reihenfolge.
    fn playlist(&self, name: &str, grenze: usize) -> Vec<Treffer>;
    fn laden(&self, deck: usize, pfad: &str) -> Result<(), String>;

    /// Schreibt die Hot Cues eines Tracks zurück.
    ///
    /// Anders als [`Sammlung::laden`] **darf** das blockieren: Es sind acht
    /// Zeilen in einer SQLite-Datei, keine Sekunden Dekodierarbeit. Dafür muss
    /// es sofort geschehen — ein Cue, der erst beim Beenden gespeichert wird,
    /// ist bei einem Absturz weg, und abgestürzt wird beim Auflegen.
    fn hot_cues_speichern(&self, pfad: &str, cues: &[(usize, f64)]) -> Result<(), String>;

    /// Schreibt ein korrigiertes Beatgrid zurück — Tempo und Anker in Sekunden.
    fn grid_speichern(&self, pfad: &str, bpm: f32, anker_sekunden: f64) -> Result<(), String>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fehler {
    UnbekanntesControl(String),
    NichtSchreibbar(String),
    FalscherTyp {
        control: String,
        erwartet: Art,
    },
    UnbekannteAuswahl {
        control: String,
        erlaubt: Vec<String>,
    },
    /// Eine Aktion lässt sich nicht wie ein Wert setzen und umgekehrt.
    IstEineAktion(String),
    IstKeineAktion(String),
    /// Die Aktion braucht ein Argument, oder es taugt nicht.
    Argument {
        control: String,
        erwartet: String,
    },
    /// Die Aktion war zulässig, ging aber schief.
    Gescheitert(String),
}

impl std::fmt::Display for Fehler {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Fehler::UnbekanntesControl(k) => write!(f, "unbekanntes Control: {k}"),
            Fehler::NichtSchreibbar(k) => write!(f, "{k} lässt sich nur lesen"),
            Fehler::FalscherTyp { control, erwartet } => {
                write!(f, "{control} erwartet {}", erwartet.name())
            }
            Fehler::UnbekannteAuswahl { control, erlaubt } => {
                write!(f, "{control} kennt nur: {}", erlaubt.join(", "))
            }
            Fehler::IstEineAktion(k) => write!(f, "{k} ist eine Aktion — mit 'do' auslösen"),
            Fehler::IstKeineAktion(k) => write!(f, "{k} ist keine Aktion — mit 'set' setzen"),
            Fehler::Argument { control, erwartet } => {
                write!(f, "{control} braucht ein Argument: {erwartet}")
            }
            Fehler::Gescheitert(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Fehler {}

/// Ein Deck, wie das Pult es sieht.
pub struct DeckEintrag {
    pub state: Arc<DeckState>,
    /// Welcher Kanalzug dieses Deck führt.
    pub kanal: usize,
    pub sample_rate: u32,
    pub frames: u64,
    pub titel: String,
    pub artist: String,
    /// Tonart des geladenen Tracks, sofern die Analyse eine gefunden hat.
    ///
    /// Sie liegt hier und nicht im `DeckState`: Der ist der Echtzeitteil und
    /// kennt nur Atomics; eine Tonart wird nie pro Sample gebraucht.
    pub tonart: Option<Tonart>,
    /// Woher der geladene Track kommt — leer, solange keiner liegt.
    ///
    /// Ohne den Pfad ließe sich nichts zurückschreiben: Die Sammlung kennt
    /// Tracks über ihn, und das Deck ist die einzige Stelle, die weiß, was
    /// gerade darauf liegt.
    pub pfad: String,
    /// Was der letzte Ladeauftrag macht: `bereit`, `laedt` oder ein Fehler.
    pub lade_status: String,
    /// Wie lang eine Phrase ist, in Beats.
    ///
    /// Nicht im `DeckState`, weil der Audio-Thread sie nie braucht — und je
    /// Deck und nicht global, weil sie eine Eigenschaft der Musik ist: Ein
    /// Stück in Achtergruppen und eines in Sechzehnern liegen gleichzeitig auf
    /// den Decks.
    pub phrase_beats: f64,
}

/// Wie lang eine Phrase standardmäßig ist.
///
/// Sechzehn Beats sind vier Takte — die Gruppe, in der House und Techno gebaut
/// sind und an deren Grenzen ein Übergang sitzt. Wer anderes auflegt, schreibt
/// `deckN.phrase_beats`.
pub const PHRASE_BEATS: f64 = 16.0;

impl DeckEintrag {
    pub fn neu(state: Arc<DeckState>, kanal: usize, sample_rate: u32) -> DeckEintrag {
        DeckEintrag {
            state,
            kanal,
            sample_rate,
            frames: 0,
            titel: String::new(),
            artist: String::new(),
            tonart: None,
            pfad: String::new(),
            lade_status: "bereit".into(),
            phrase_beats: PHRASE_BEATS,
        }
    }
}

/// Gespiegelte Reglerstellung eines Kanalzuges.
#[derive(Debug, Clone, PartialEq)]
pub struct KanalSpiegel {
    pub name: String,
    pub trim: f64,
    pub eq_low: f64,
    pub eq_mid: f64,
    pub eq_high: f64,
    pub filter: f64,
    pub fader: f64,
    pub cue: bool,
    pub assign: Assign,
    pub fx: Effekt,
    pub fx_mix: f64,
    pub fx_amount: f64,
    pub fx_time: f64,
}

impl KanalSpiegel {
    pub fn neu(name: impl Into<String>, assign: Assign) -> KanalSpiegel {
        KanalSpiegel {
            name: name.into(),
            trim: 1.0,
            eq_low: 1.0,
            eq_mid: 1.0,
            eq_high: 1.0,
            filter: 0.0,
            // Zu: ein Kanal, der beim Start auf die Anlage geht, ist ein Unfall.
            fader: 0.0,
            cue: false,
            assign,
            fx: Effekt::Aus,
            fx_mix: 0.0,
            fx_amount: 0.5,
            fx_time: 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterSpiegel {
    pub crossfader: f64,
    pub crossfader_curve: f64,
    pub gain: f64,
    pub cue_gain: f64,
    pub cue_mix: f64,
}

impl Default for MasterSpiegel {
    fn default() -> Self {
        MasterSpiegel {
            crossfader: 0.0,
            crossfader_curve: 0.0,
            gain: 1.0,
            cue_gain: 1.0,
            cue_mix: 0.0,
        }
    }
}

pub struct Steuerpult {
    /// Was später geschehen soll — siehe [`crate::zeitplan`].
    pub plan: crate::zeitplan::Zeitplan,
    /// Was als Nächstes gespielt werden soll — siehe [`crate::warteschlange`].
    pub liste: crate::warteschlange::Warteschlange,
    /// Was von außen hereinkommt — siehe [`crate::signal`].
    pub signale: [crate::signal::Signal; crate::signal::SIGNALE],
    decks: Vec<DeckEintrag>,
    kanaele: Vec<KanalSpiegel>,
    master: MasterSpiegel,
    handle: EngineHandle,
    sammlung: Option<Box<dyn Sammlung>>,
    aufnahme: Option<Aufnahme>,
}

impl Steuerpult {
    pub fn neu(handle: EngineHandle) -> Steuerpult {
        Steuerpult {
            plan: crate::zeitplan::Zeitplan::neu(),
            liste: crate::warteschlange::Warteschlange::neu(),
            signale: std::array::from_fn(|_| crate::signal::Signal::neu()),
            decks: Vec::new(),
            kanaele: Vec::new(),
            master: MasterSpiegel::default(),
            handle,
            sammlung: None,
            aufnahme: None,
        }
    }

    /// Hängt den Mitschnitt an. Ohne das antwortet `record` mit einem Fehler,
    /// statt stillschweigend nichts aufzunehmen.
    pub fn aufnahme_setzen(&mut self, aufnahme: Aufnahme) {
        self.aufnahme = Some(aufnahme);
    }

    /// Hängt Suche und Laden an. Ohne das antworten beide mit einem Fehler,
    /// statt stillschweigend nichts zu tun.
    pub fn sammlung_setzen(&mut self, sammlung: Box<dyn Sammlung>) {
        self.sammlung = Some(sammlung);
    }

    pub fn deck_hinzufuegen(&mut self, eintrag: DeckEintrag) -> usize {
        self.decks.push(eintrag);
        self.decks.len() - 1
    }

    pub fn kanal_hinzufuegen(&mut self, spiegel: KanalSpiegel) -> usize {
        self.kanaele.push(spiegel);
        self.kanaele.len() - 1
    }

    pub fn decks(&self) -> &[DeckEintrag] {
        &self.decks
    }

    pub fn deck_mut(&mut self, index: usize) -> Option<&mut DeckEintrag> {
        self.decks.get_mut(index)
    }

    pub fn kanaele(&self) -> &[KanalSpiegel] {
        &self.kanaele
    }

    pub fn master(&self) -> &MasterSpiegel {
        &self.master
    }

    pub fn handle_mut(&mut self) -> &mut EngineHandle {
        &mut self.handle
    }

    /// Alle Controls, die es gerade gibt — mit Beschreibung.
    ///
    /// „Gerade" ist wörtlich zu nehmen: Die Liste wächst mit den angemeldeten
    /// Decks und Kanälen. Ein Agent, der zwei Decks sieht, bekommt zwei Decks
    /// aufgezählt, nicht die theoretisch möglichen vier.
    pub fn liste(&self) -> Vec<(Schluessel, Beschreibung)> {
        let mut aus = Vec::new();

        for (i, _) in self.decks.iter().enumerate() {
            for b in katalog::DECK {
                aus.push((Schluessel::neu(Gruppe::Deck(i), b.element), b.clone()));
            }
            for c in 0..katalog::HOT_CUES {
                if let Some(b) = katalog::hot_cue_beschreibung(c) {
                    aus.push((Schluessel::neu(Gruppe::Deck(i), b.element), b));
                }
            }
        }

        for (i, _) in self.kanaele.iter().enumerate() {
            for b in katalog::KANAL {
                aus.push((Schluessel::neu(Gruppe::Kanal(i), b.element), b.clone()));
            }
        }

        for b in katalog::MASTER {
            aus.push((Schluessel::neu(Gruppe::Master, b.element), b.clone()));
        }

        aus
    }

    /// Beschreibung eines einzelnen Controls, sofern es das gibt.
    pub fn beschreibung(&self, k: &Schluessel) -> Option<Beschreibung> {
        match k.gruppe {
            Gruppe::Deck(i) if i < self.decks.len() => katalog::DECK
                .iter()
                .find(|b| b.element == k.element)
                .cloned()
                .or_else(|| {
                    self.hot_cue_index(&k.element)
                        .and_then(katalog::hot_cue_beschreibung)
                }),
            Gruppe::Kanal(i) if i < self.kanaele.len() => katalog::KANAL
                .iter()
                .find(|b| b.element == k.element)
                .cloned(),
            Gruppe::Master => katalog::MASTER
                .iter()
                .find(|b| b.element == k.element)
                .cloned(),
            _ => None,
        }
    }

    fn hot_cue_index(&self, element: &str) -> Option<usize> {
        let rest = element.strip_prefix("cue")?;
        let nummer: usize = rest.parse().ok()?;
        nummer.checked_sub(1).filter(|i| *i < katalog::HOT_CUES)
    }

    pub fn lies(&self, k: &Schluessel) -> Result<Wert, Fehler> {
        match self.beschreibung(k) {
            None => return Err(Fehler::UnbekanntesControl(k.to_string())),
            // Ein Auslöser hat keinen Zustand. Etwas zurückzugeben hieße,
            // etwas zu erfinden.
            Some(b) if b.art == Art::Aktion => return Err(Fehler::IstEineAktion(k.to_string())),
            Some(_) => {}
        }

        match k.gruppe {
            Gruppe::Deck(i) => self.lies_deck(i, &k.element),
            Gruppe::Kanal(i) => self.lies_kanal(i, &k.element),
            Gruppe::Master => self.lies_master(&k.element),
        }
        .ok_or_else(|| Fehler::UnbekanntesControl(k.to_string()))
    }

    fn lies_deck(&self, i: usize, element: &str) -> Option<Wert> {
        let d = self.decks.get(i)?;
        let rate = d.sample_rate as f64;
        let sek = |frames: u64| Wert::Zahl(frames as f64 / rate);

        let wert = match element {
            "play" => Wert::Schalter(d.state.is_playing()),
            "position" => sek(d.state.position_frames()),
            "duration" => sek(d.frames),
            "bpm" => match d.state.effective_bpm() {
                Some(b) => Wert::Zahl(b as f64),
                None => Wert::Leer,
            },
            "bpm_grid" => match d.state.grid() {
                Some(g) => Wert::Zahl(g.bpm as f64),
                None => Wert::Leer,
            },
            "grid_anchor" => match d.state.grid() {
                Some(g) => sek(g.anchor_frames),
                None => Wert::Leer,
            },
            "tempo" => Wert::Zahl(d.state.tempo() as f64),
            "keylock" => Wert::Schalter(d.state.keylock()),
            "beat_phase" => match d.state.beat_phase(d.sample_rate) {
                Some(p) => Wert::Zahl(p),
                None => Wert::Leer,
            },
            "beat" => match d.state.grid() {
                Some(g) => Wert::Zahl(g.beat_at(d.state.position_frames() as f64, d.sample_rate)),
                None => Wert::Leer,
            },
            // Grid-Beats, also ohne den Tempo-Regler. Ein Beat ist eine feste
            // Zahl Quell-Frames; schneller abgespielt vergeht er in weniger
            // Zeit, aber es werden davon nicht mehr. Genauso rechnen `in` und
            // `ramp` — eine zweite Zeitrechnung im selben Steuerraum wäre eine
            // Falle. Wer Sekunden will, teilt selbst durch das Tempo.
            "beats_left" => match (d.state.grid(), d.frames) {
                (Some(g), ende) if ende > 0 => {
                    let jetzt = d.state.position_frames();
                    let pro_beat = g.frames_per_beat(d.sample_rate);
                    Wert::Zahl(ende.saturating_sub(jetzt) as f64 / pro_beat)
                }
                _ => Wert::Leer,
            },
            "phrase_beats" => Wert::Zahl(d.phrase_beats),
            "beats_to_phrase" => match d.state.grid() {
                Some(g) => {
                    let beat = g.beat_at(d.state.position_frames() as f64, d.sample_rate);
                    let laenge = d.phrase_beats.max(1.0);
                    // Rest bis zur nächsten Grenze; steht man genau darauf, ist
                    // es 0 und nicht eine ganze Phrase.
                    Wert::Zahl((laenge - beat.rem_euclid(laenge)).rem_euclid(laenge))
                }
                None => Wert::Leer,
            },
            "loop_active" => Wert::Schalter(d.state.is_looping()),
            "loop_beats" => match d.state.loop_range() {
                // Aus der Schleifenlänge zurückgerechnet: Das Deck merkt sich
                // Frames, nicht Beats.
                Some((a, b)) => match d.state.grid() {
                    Some(g) => Wert::Zahl((b - a) as f64 / g.frames_per_beat(d.sample_rate)),
                    None => Wert::Leer,
                },
                None => Wert::Leer,
            },
            "title" => Wert::Text(d.titel.clone()),
            "artist" => Wert::Text(d.artist.clone()),
            // Zwei Felder statt eines: Menschen lesen `Am`, gemischt wird nach
            // `8A`. Beides in eine Zeichenkette zu packen hieße, dass jeder
            // Leser sie wieder auseinandernehmen muss.
            "key" => match d.tonart {
                Some(t) => Wert::Text(t.name()),
                None => Wert::Leer,
            },
            "key_camelot" => match d.tonart {
                Some(t) => Wert::Text(t.camelot()),
                None => Wert::Leer,
            },
            "finished" => Wert::Schalter(d.state.is_finished()),
            "load_status" => Wert::Text(d.lade_status.clone()),
            _ => {
                let c = self.hot_cue_index(element)?;
                match d.state.cue(c) {
                    Some(f) => sek(f),
                    None => Wert::Leer,
                }
            }
        };

        Some(wert)
    }

    fn lies_kanal(&self, i: usize, element: &str) -> Option<Wert> {
        let k = self.kanaele.get(i)?;
        let wert = match element {
            "trim" => Wert::Zahl(k.trim),
            "eq_low" => Wert::Zahl(k.eq_low),
            "eq_mid" => Wert::Zahl(k.eq_mid),
            "eq_high" => Wert::Zahl(k.eq_high),
            "filter" => Wert::Zahl(k.filter),
            "fader" => Wert::Zahl(k.fader),
            "cue" => Wert::Schalter(k.cue),
            "assign" => Wert::Auswahl(assign_name(k.assign).to_string()),
            "fx" => Wert::Auswahl(k.fx.name().to_string()),
            "fx_mix" => Wert::Zahl(k.fx_mix),
            "fx_amount" => Wert::Zahl(k.fx_amount),
            "fx_time" => Wert::Zahl(k.fx_time),
            _ => return None,
        };
        Some(wert)
    }

    /// Zerlegt `signal2_trend` in Platz und Feld.
    fn signal_teile(element: &str) -> Option<(usize, &'static str)> {
        let rest = element.strip_prefix("signal")?;
        let (zahl, feld) = match rest.find('_') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let nummer: usize = zahl.parse().ok()?;
        if nummer == 0 || nummer > crate::signal::SIGNALE {
            return None;
        }
        let feld = match feld {
            "" => "wert",
            "_name" => "name",
            "_trend" => "trend",
            "_age" => "age",
            _ => return None,
        };
        Some((nummer - 1, feld))
    }

    fn lies_signal(&self, element: &str) -> Option<Wert> {
        let (i, feld) = Self::signal_teile(element)?;
        let s = &self.signale[i];
        // Einmal genommen und für alle Felder benutzt: Sonst lägen `wert` und
        // `age` desselben Signals an minimal verschiedenen Zeitpunkten.
        let jetzt = std::time::Instant::now();

        let wert = match feld {
            "name" => Wert::Text(s.name.clone()),
            "wert" => match s.wert() {
                Some(v) => Wert::Zahl(v),
                None => Wert::Leer,
            },
            "trend" => match s.trend(jetzt) {
                Some(v) => Wert::Zahl(v),
                None => Wert::Leer,
            },
            _ => match s.alter(jetzt) {
                Some(v) => Wert::Zahl(v),
                None => Wert::Leer,
            },
        };
        Some(wert)
    }

    fn lies_master(&self, element: &str) -> Option<Wert> {
        if element.starts_with("signal") {
            return self.lies_signal(element);
        }
        let m = &self.master;

        // Der Mitschnitt hat seinen eigenen Zustand, nicht den des Mixers.
        match element {
            "recording" => {
                return Some(Wert::Schalter(
                    self.aufnahme.as_ref().is_some_and(|a| a.laeuft()),
                ))
            }
            "record_seconds" => {
                return Some(Wert::Zahl(
                    self.aufnahme.as_ref().map(|a| a.sekunden()).unwrap_or(0.0),
                ))
            }
            "record_dropped" => {
                return Some(Wert::Zahl(
                    self.aufnahme.as_ref().map(|a| a.verworfen()).unwrap_or(0) as f64,
                ))
            }
            _ => {}
        }

        let wert = match element {
            "crossfader" => Wert::Zahl(m.crossfader),
            "crossfader_curve" => Wert::Zahl(m.crossfader_curve),
            "gain" => Wert::Zahl(m.gain),
            "cue_gain" => Wert::Zahl(m.cue_gain),
            "cue_mix" => Wert::Zahl(m.cue_mix),
            _ => return None,
        };
        Some(wert)
    }

    /// Löst eine Aktion aus.
    ///
    /// Antwortet mit Zeilen, die der Aufrufer zu sehen bekommt — `sync` meldet
    /// zurück, wie weit die Phase danebenlag, `search` gibt seine Treffer aus.
    /// Ein leerer Rückgabewert heißt schlicht: hat geklappt.
    pub fn ausloesen(
        &mut self,
        k: &Schluessel,
        argument: Option<&str>,
    ) -> Result<Vec<String>, Fehler> {
        let Some(b) = self.beschreibung(k) else {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
        };
        if b.art != Art::Aktion {
            return Err(Fehler::IstKeineAktion(k.to_string()));
        }

        let fehlt = || Fehler::Argument {
            control: k.to_string(),
            erwartet: b.argument.to_string(),
        };

        match (k.gruppe, k.element.as_str()) {
            (Gruppe::Deck(i), "sync") => self.sync(i, argument),
            (Gruppe::Deck(i), "load") => {
                let pfad = argument.ok_or_else(fehlt)?;
                self.laden(i, pfad)
            }
            (Gruppe::Deck(i), "jump_cue") => {
                let nummer: usize = argument.and_then(|a| a.parse().ok()).ok_or_else(fehlt)?;
                self.hot_cue_anspringen(i, nummer)
            }
            (Gruppe::Deck(i), "beatjump") => {
                let beats: f64 = argument.and_then(|a| a.parse().ok()).ok_or_else(fehlt)?;
                self.beatjump(i, beats)
            }
            (Gruppe::Deck(i), "grid_here") => self.grid_hierher(i),
            (Gruppe::Deck(i), "grid_scale") => {
                let faktor: f64 = argument.and_then(|a| a.parse().ok()).ok_or_else(fehlt)?;
                self.grid_skalieren(i, faktor)
            }
            (Gruppe::Kanal(i), "fx_sync") => {
                let beats: f64 = argument.and_then(|a| a.parse().ok()).ok_or_else(fehlt)?;
                self.fx_sync(i, beats)
            }
            (Gruppe::Master, "record") => {
                let pfad = argument.ok_or_else(fehlt)?;
                self.aufnehmen(pfad)
            }
            (Gruppe::Master, "record_stop") => self.aufnahme_stoppen(),
            (Gruppe::Master, "search") => {
                let treffer = self.suche(argument.unwrap_or(""));
                Ok(treffer_zeilen(&treffer))
            }
            (Gruppe::Master, "playlists") => {
                let namen = self.playlists();
                if namen.is_empty() {
                    return Ok(vec!["hinweis keine Playlists in der Sammlung".into()]);
                }
                Ok(namen.into_iter().map(|n| format!("playlist {n}")).collect())
            }
            (Gruppe::Master, "playlist") => {
                let name = argument.ok_or_else(fehlt)?;
                let treffer = self.playlist(name);
                if treffer.is_empty() {
                    return Err(Fehler::Gescheitert(format!(
                        "Playlist '{name}' ist leer oder gibt es nicht"
                    )));
                }
                Ok(treffer_zeilen(&treffer))
            }
            (Gruppe::Master, "search_mixable") => {
                let bpm: f32 = argument.and_then(|a| a.parse().ok()).ok_or_else(fehlt)?;
                let treffer = self.suche_mischbar(bpm);
                Ok(treffer_zeilen(&treffer))
            }
            (Gruppe::Master, "search_harmonic") => {
                let roh = argument.ok_or_else(fehlt)?;
                // Eine unlesbare Tonart muss auffallen. Sie stillschweigend
                // als „keine Treffer" zu beantworten hieße, einen Tippfehler
                // wie eine leere Sammlung aussehen zu lassen.
                let tonart = Tonart::parse(roh).ok_or_else(|| Fehler::Argument {
                    control: k.to_string(),
                    erwartet: format!("eine Tonart wie Am, F# oder 8A — '{roh}' ist keine"),
                })?;
                let treffer = self.suche_harmonisch(tonart);
                Ok(treffer_zeilen(&treffer))
            }
            (Gruppe::Master, "queue") => Ok(self.listen_zeilen()),
            (Gruppe::Master, "queue_add") => {
                let pfad = argument.ok_or_else(fehlt)?;
                self.liste_anhaengen(pfad)
            }
            (Gruppe::Master, "queue_note") => {
                let roh = argument.ok_or_else(fehlt)?;
                let (nr, text) = roh.split_once(char::is_whitespace).ok_or_else(fehlt)?;
                let nr = Self::listen_nummer(k, nr)?;
                if !self.liste.notieren(nr, text.trim().to_string()) {
                    return Err(Fehler::Gescheitert(format!(
                        "Nummer {nr} steht nicht in der Liste"
                    )));
                }
                Ok(vec![format!("queue {nr} notiert")])
            }
            (Gruppe::Master, "queue_bump") => {
                let nr = Self::listen_nummer(k, argument.ok_or_else(fehlt)?)?;
                if !self.liste.vorziehen(nr) {
                    return Err(Fehler::Gescheitert(format!(
                        "Nummer {nr} steht nicht in der Liste"
                    )));
                }
                Ok(vec![format!("queue {nr} ist jetzt der Naechste")])
            }
            (Gruppe::Master, "queue_drop") => {
                let nr = Self::listen_nummer(k, argument.ok_or_else(fehlt)?)?;
                match self.liste.streichen(nr) {
                    Some(e) => Ok(vec![format!("queue {nr} gestrichen {}", e.pfad)]),
                    None => Err(Fehler::Gescheitert(format!(
                        "Nummer {nr} steht nicht in der Liste"
                    ))),
                }
            }
            (Gruppe::Master, "queue_clear") => {
                Ok(vec![format!("ok {} gestrichen", self.liste.leeren())])
            }
            (Gruppe::Master, "queue_next") => self.liste_auflegen(k, argument),
            _ => Err(Fehler::UnbekanntesControl(k.to_string())),
        }
    }

    /// Zieht ein Deck auf ein anderes — Tempo **und** Phase.
    ///
    /// Ohne Argument das jeweils andere, was bei zwei Decks das Erwartbare
    /// ist. Bei mehr Decks muss man sagen, welches gemeint ist, statt dass
    /// eine Reihenfolge entscheidet.
    fn sync(&mut self, slave: usize, argument: Option<&str>) -> Result<Vec<String>, Fehler> {
        let master = match argument {
            Some(name) => match Gruppe::parse(name) {
                Some(Gruppe::Deck(i)) => i,
                _ => {
                    return Err(Fehler::Argument {
                        control: format!("deck{}.sync", slave + 1),
                        erwartet: "ein Deck, etwa deck1".into(),
                    })
                }
            },
            None if self.decks.len() == 2 => 1 - slave.min(1),
            None => {
                return Err(Fehler::Argument {
                    control: format!("deck{}.sync", slave + 1),
                    erwartet: "bei mehr als zwei Decks das Ziel benennen".into(),
                })
            }
        };

        if master == slave {
            return Err(Fehler::Gescheitert(
                "ein Deck lässt sich nicht auf sich selbst ziehen".into(),
            ));
        }

        let (Some(m), Some(s)) = (self.decks.get(master), self.decks.get(slave)) else {
            return Err(Fehler::UnbekanntesControl(format!("deck{}", master + 1)));
        };
        let rate = s.sample_rate;

        match audio_engine::sync(&m.state, &s.state, rate) {
            Some(plan) => Ok(vec![format!(
                "sync deck{} auf deck{} tempo {:.5} phase {:+.4}",
                slave + 1,
                master + 1,
                plan.tempo,
                plan.phase_error_beats
            )]),
            None => Err(Fehler::Gescheitert(
                "Sync braucht auf beiden Decks ein Beatgrid".into(),
            )),
        }
    }

    fn laden(&mut self, deck: usize, pfad: &str) -> Result<Vec<String>, Fehler> {
        if deck >= self.decks.len() {
            return Err(Fehler::UnbekanntesControl(format!("deck{}", deck + 1)));
        }
        let Some(sammlung) = self.sammlung.as_ref() else {
            return Err(Fehler::Gescheitert(
                "keine Sammlung angeschlossen — Laden geht nicht".into(),
            ));
        };

        sammlung.laden(deck, pfad).map_err(Fehler::Gescheitert)?;
        // Der Auftrag ist angenommen, nicht erledigt. Wer wissen will, wann
        // der Track liegt, fragt `load_status` oder abonniert es.
        self.decks[deck].lade_status = "laedt".into();
        Ok(vec![format!("load deck{} angenommen", deck + 1)])
    }

    /// Die Liste als Zeilen: `queue <nr> <pfad> <notiz>`.
    ///
    /// Pfad vor Notiz und getrennt an der Dateiendung — dieselbe Form wie bei
    /// den Suchtreffern, damit ein Leser nur eine Regel braucht.
    fn listen_zeilen(&self) -> Vec<String> {
        if self.liste.ist_leer() {
            return vec!["hinweis die Liste ist leer".into()];
        }
        self.liste
            .eintraege()
            .iter()
            .map(|e| {
                let notiz = if e.notiz.is_empty() { "-" } else { &e.notiz };
                format!("queue {} {} {notiz}", e.id, e.pfad)
            })
            .collect()
    }

    fn listen_nummer(k: &Schluessel, roh: &str) -> Result<u64, Fehler> {
        roh.trim().parse::<u64>().map_err(|_| Fehler::Argument {
            control: k.to_string(),
            erwartet: format!("eine Nummer aus der Liste — '{}' ist keine", roh.trim()),
        })
    }

    fn liste_anhaengen(&mut self, pfad: &str) -> Result<Vec<String>, Fehler> {
        match self.liste.anhaengen(pfad.to_string(), String::new()) {
            Ok(nr) => Ok(vec![format!("queue {nr} angehaengt {pfad}")]),
            Err(schon) => Err(Fehler::Gescheitert(format!(
                "steht schon als Nummer {schon} in der Liste"
            ))),
        }
    }

    /// Ein Deck, auf das gefahrlos geladen werden kann.
    ///
    /// Zuerst ein durchgelaufenes, sonst irgendeines, das steht.
    fn freies_deck(&self) -> Option<usize> {
        let steht = |i: &usize| !self.decks[*i].state.is_playing();
        (0..self.decks.len())
            .find(|i| steht(i) && self.decks[*i].state.is_finished())
            .or_else(|| (0..self.decks.len()).find(steht))
    }

    /// Legt den vordersten Eintrag auf.
    ///
    /// Ohne Deckangabe auf eines, das gerade nicht läuft. Laufen alle, wird das
    /// gesagt statt geraten: Ein Track, der über einen laufenden gelegt wird,
    /// reißt den Mix ab, und das ist keine Entscheidung, die eine Vorgabe
    /// treffen darf.
    fn liste_auflegen(
        &mut self,
        k: &Schluessel,
        argument: Option<&str>,
    ) -> Result<Vec<String>, Fehler> {
        let deck = match argument {
            Some(name) => match Gruppe::parse(name) {
                Some(Gruppe::Deck(i)) if i < self.decks.len() => i,
                _ => {
                    return Err(Fehler::Argument {
                        control: k.to_string(),
                        erwartet: format!("ein Deck, etwa deck1 — '{name}' ist keins"),
                    })
                }
            },
            None => self.freies_deck().ok_or_else(|| {
                Fehler::Gescheitert(
                    "alle Decks laufen — Deck nennen, wenn wirklich darüber gelegt werden soll"
                        .into(),
                )
            })?,
        };

        let Some(eintrag) = self.liste.abnehmen() else {
            return Err(Fehler::Gescheitert("die Liste ist leer".into()));
        };

        // Scheitert das Laden, kommt der Eintrag zurück nach vorn. Sonst wäre
        // er weg, ohne gespielt worden zu sein.
        match self.laden(deck, &eintrag.pfad) {
            Ok(mut zeilen) => {
                zeilen.push(format!("queue {} abgenommen {}", eintrag.id, eintrag.pfad));
                if !eintrag.notiz.is_empty() {
                    zeilen.push(format!("notiz {}", eintrag.notiz));
                }
                Ok(zeilen)
            }
            Err(e) => {
                self.liste.zuruecklegen(eintrag);
                Err(e)
            }
        }
    }

    fn hot_cue_anspringen(&mut self, deck: usize, nummer: usize) -> Result<Vec<String>, Fehler> {
        let Some(d) = self.decks.get(deck) else {
            return Err(Fehler::UnbekanntesControl(format!("deck{}", deck + 1)));
        };
        let Some(index) = nummer.checked_sub(1).filter(|i| *i < katalog::HOT_CUES) else {
            return Err(Fehler::Argument {
                control: format!("deck{}.jump_cue", deck + 1),
                erwartet: format!("1 bis {}", katalog::HOT_CUES),
            });
        };

        // Ein ungesetzter Cue springt nirgendwohin — das stumm an den Anfang
        // zu deuten wäre auf einer Anlage ein Unfall.
        if d.state.cue(index).is_none() {
            return Err(Fehler::Gescheitert(format!(
                "deck{}.cue{nummer} ist nicht gesetzt",
                deck + 1
            )));
        }

        d.state.jump_to_cue(index);
        Ok(Vec::new())
    }

    /// Setzt die Effektzeit auf so viele Beats des zugehörigen Decks.
    ///
    /// Ein Delay, das nicht im Takt steht, klingt nach Fehler. Die Umrechnung
    /// braucht das Beatgrid, und das kennt nur das Deck — deshalb liegt sie
    /// hier und nicht im Mixer, der von Decks nichts weiß.
    fn fx_sync(&mut self, kanal: usize, beats: f64) -> Result<Vec<String>, Fehler> {
        if beats <= 0.0 {
            return Err(Fehler::Argument {
                control: format!("channel{}.fx_sync", kanal + 1),
                erwartet: "eine positive Zahl von Beats".into(),
            });
        }

        // Welches Deck hängt an diesem Zug? Ein AUX-Kanal hat keins.
        let Some(deck) = self.decks.iter().find(|d| d.kanal == kanal) else {
            return Err(Fehler::Gescheitert(format!(
                "an channel{} hängt kein Deck — fx_time direkt setzen",
                kanal + 1
            )));
        };
        let Some(bpm) = deck.state.effective_bpm() else {
            return Err(Fehler::Gescheitert(
                "das Deck hat kein Beatgrid — fx_time direkt setzen".into(),
            ));
        };

        let sekunden = beats * 60.0 / bpm as f64;
        let schluessel = Schluessel::neu(Gruppe::Kanal(kanal), "fx_time");
        self.schreibe(&schluessel, Wert::Zahl(sekunden))?;

        Ok(vec![format!(
            "fx_sync channel{} {beats} Beats bei {bpm:.2} BPM = {sekunden:.4} s",
            kanal + 1
        )])
    }

    fn beatjump(&mut self, deck: usize, beats: f64) -> Result<Vec<String>, Fehler> {
        let Some(d) = self.decks.get(deck) else {
            return Err(Fehler::UnbekanntesControl(format!("deck{}", deck + 1)));
        };
        let Some(grid) = d.state.grid() else {
            return Err(Fehler::Gescheitert("Beatjump braucht ein Beatgrid".into()));
        };

        let je_beat = grid.frames_per_beat(d.sample_rate);
        let jetzt = d.state.position_frames() as f64;
        let ziel = (jetzt + beats * je_beat).clamp(0.0, d.frames as f64);
        d.state.seek_frames(ziel as u64);
        Ok(Vec::new())
    }

    fn aufnehmen(&mut self, pfad: &str) -> Result<Vec<String>, Fehler> {
        let Some(aufnahme) = self.aufnahme.as_mut() else {
            return Err(Fehler::Gescheitert("kein Mitschnitt angeschlossen".into()));
        };

        aufnahme
            .starten(std::path::Path::new(pfad))
            .map_err(Fehler::Gescheitert)?;
        Ok(vec![format!("record läuft nach {pfad}")])
    }

    fn aufnahme_stoppen(&mut self) -> Result<Vec<String>, Fehler> {
        let Some(aufnahme) = self.aufnahme.as_mut() else {
            return Err(Fehler::Gescheitert("kein Mitschnitt angeschlossen".into()));
        };

        let sekunden = aufnahme.sekunden();
        let verworfen = aufnahme.verworfen();
        let Some(pfad) = aufnahme.stoppen() else {
            return Err(Fehler::Gescheitert("es läuft kein Mitschnitt".into()));
        };

        let mut zeilen = vec![format!("record {} · {sekunden:.1} s", pfad.display())];
        // Lücken müssen dastehen, nicht erst beim Anhören auffallen.
        if verworfen > 0 {
            zeilen.push(format!(
                "warnung {verworfen} Frames fehlen — der Mitschnitt hat Lücken"
            ));
        }
        Ok(zeilen)
    }

    /// Freitextsuche. Auch die Oberfläche geht hier durch — zwei Suchwege
    /// wären zwei Stellen, an denen sich das Ergebnis unterscheiden könnte.
    pub fn suche(&self, text: &str) -> Vec<Treffer> {
        match self.sammlung.as_ref() {
            Some(s) => s.suchen(text, GRENZE),
            None => Vec::new(),
        }
    }

    pub fn suche_mischbar(&self, bpm: f32) -> Vec<Treffer> {
        match self.sammlung.as_ref() {
            Some(s) => s.suchen_mischbar(bpm, GRENZE),
            None => Vec::new(),
        }
    }

    /// Tracks, deren Tonart harmonisch zu `tonart` passt.
    pub fn suche_harmonisch(&self, tonart: Tonart) -> Vec<Treffer> {
        match self.sammlung.as_ref() {
            Some(s) => s.suchen_harmonisch(tonart, GRENZE),
            None => Vec::new(),
        }
    }

    /// Tonart eines Decks — der übliche Ausgangspunkt für die harmonische
    /// Suche.
    pub fn deck_tonart(&self, deck: usize) -> Option<Tonart> {
        self.decks.get(deck).and_then(|d| d.tonart)
    }

    /// Namen aller Playlists.
    pub fn playlists(&self) -> Vec<String> {
        match self.sammlung.as_ref() {
            Some(s) => s.playlists(),
            None => Vec::new(),
        }
    }

    /// Die Tracks einer Playlist.
    pub fn playlist(&self, name: &str) -> Vec<Treffer> {
        match self.sammlung.as_ref() {
            Some(s) => s.playlist(name, GRENZE),
            None => Vec::new(),
        }
    }

    pub fn schreibe(&mut self, k: &Schluessel, wert: Wert) -> Result<(), Fehler> {
        let Some(b) = self.beschreibung(k) else {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
        };
        if b.art == Art::Aktion {
            return Err(Fehler::IstEineAktion(k.to_string()));
        }
        if !b.schreibbar {
            return Err(Fehler::NichtSchreibbar(k.to_string()));
        }

        match k.gruppe {
            Gruppe::Deck(i) => self.schreibe_deck(i, &b, k, wert),
            Gruppe::Kanal(i) => self.schreibe_kanal(i, &b, k, wert),
            Gruppe::Master => self.schreibe_master(&b, k, wert),
        }
    }

    /// Schreiben mit einem Wert aus 0..1, in den echten Bereich gedehnt.
    ///
    /// Das ist der Weg, den ein MIDI-Controller nimmt: Er kennt nur 0 bis 127
    /// und weiß nichts über Faktoren oder Bereiche.
    pub fn schreibe_normiert(&mut self, k: &Schluessel, norm: f64) -> Result<(), Fehler> {
        let Some(b) = self.beschreibung(k) else {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
        };

        let wert = match b.art {
            Art::Schalter => Wert::Schalter(norm >= 0.5),
            _ => Wert::Zahl(b.aus_normiert(norm)),
        };
        self.schreibe(k, wert)
    }

    fn zahl(b: &Beschreibung, k: &Schluessel, wert: &Wert) -> Result<f64, Fehler> {
        wert.als_zahl()
            .map(|v| b.begrenzen(v))
            .ok_or_else(|| Fehler::FalscherTyp {
                control: k.to_string(),
                erwartet: b.art,
            })
    }

    fn schalter(b: &Beschreibung, k: &Schluessel, wert: &Wert) -> Result<bool, Fehler> {
        wert.als_schalter().ok_or_else(|| Fehler::FalscherTyp {
            control: k.to_string(),
            erwartet: b.art,
        })
    }

    fn schreibe_deck(
        &mut self,
        i: usize,
        b: &Beschreibung,
        k: &Schluessel,
        wert: Wert,
    ) -> Result<(), Fehler> {
        let Some(d) = self.decks.get(i) else {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
        };
        let rate = d.sample_rate as f64;

        match k.element.as_str() {
            "play" => d.state.set_playing(Self::schalter(b, k, &wert)?),
            "keylock" => d.state.set_keylock(Self::schalter(b, k, &wert)?),
            "phrase_beats" => {
                let laenge = Self::zahl(b, k, &wert)?;
                // Über den Index und nicht über `d`: `d` ist nur geliehen, und
                // die Phrasenlänge liegt im Eintrag, nicht im DeckState.
                self.decks[i].phrase_beats = laenge;
                return Ok(());
            }
            "loop_active" => {
                d.state.set_loop_active(Self::schalter(b, k, &wert)?);
            }
            "tempo" => d.state.set_tempo(Self::zahl(b, k, &wert)? as f32),
            "position" => d
                .state
                .seek_frames((Self::zahl(b, k, &wert)? * rate).max(0.0) as u64),
            "loop_beats" => {
                let beats = Self::zahl(b, k, &wert)?;
                // Ohne Grid gibt es keine Beats — dann passiert nichts, statt
                // eine Schleife an einer geratenen Stelle zu setzen.
                if d.state.set_loop_beats(beats, d.sample_rate) {
                    d.state.set_loop_active(true);
                }
            }
            "bpm_grid" => {
                let bpm = Self::zahl(b, k, &wert)? as f32;
                // Der Anker bleibt stehen: Wer das Tempo korrigiert, meint fast
                // immer den Oktavfehler, nicht die Lage der Eins.
                let anker = d.state.grid().map(|g| g.anchor_frames).unwrap_or(0);
                d.state.set_grid(Some(Beatgrid::new(bpm, anker, 1.0)));
                self.grid_sichern(i);
            }
            "grid_anchor" => {
                let sekunden = Self::zahl(b, k, &wert)?;
                let Some(vorher) = d.state.grid() else {
                    return Err(Fehler::Gescheitert(
                        "ohne Tempo kein Anker — erst bpm_grid setzen".into(),
                    ));
                };
                let frames = (sekunden * rate).max(0.0) as u64;
                d.state
                    .set_grid(Some(Beatgrid::new(vorher.bpm, frames, 1.0)));
                self.grid_sichern(i);
            }
            _ => {
                let Some(c) = self.hot_cue_index(&k.element) else {
                    return Err(Fehler::UnbekanntesControl(k.to_string()));
                };
                match wert {
                    // Ein Hot Cue lässt sich löschen, indem man ihn leert.
                    Wert::Leer => d.state.set_cue(c, None),
                    _ => {
                        let sek = Self::zahl(b, k, &wert)?;
                        d.state.set_cue(c, Some((sek * rate).max(0.0) as u64));
                    }
                }
                // Sofort zurückschreiben, nicht beim Beenden: Ein Cue, der nur
                // im Speicher steht, ist nach einem Absturz weg — und abgestürzt
                // wird beim Auflegen.
                self.cues_sichern(i);
            }
        }

        Ok(())
    }

    /// Legt den Anker auf die aktuelle Abspielposition.
    ///
    /// Der klassische Handgriff: Man hört, wo die Eins liegt, und sagt es dem
    /// Raster. Das Tempo bleibt, wie es war — verschoben wird nur die Phase.
    fn grid_hierher(&mut self, deck: usize) -> Result<Vec<String>, Fehler> {
        let Some(d) = self.decks.get(deck) else {
            return Err(Fehler::UnbekanntesControl(format!("deck{}", deck + 1)));
        };
        let Some(grid) = d.state.grid() else {
            return Err(Fehler::Gescheitert(
                "ohne Tempo kein Anker — erst bpm_grid setzen".into(),
            ));
        };

        // Das Ziel, nicht die Anzeige: Wer eben gesprungen ist und dann sagt
        // „hier ist die Eins", meint die Stelle, auf die er gesprungen ist.
        let position = d.state.ziel_position();
        let sekunden = position as f64 / d.sample_rate as f64;
        d.state
            .set_grid(Some(Beatgrid::new(grid.bpm, position, 1.0)));
        self.grid_sichern(deck);

        Ok(vec![format!(
            "grid deck{} Anker auf {sekunden:.3} s",
            deck + 1
        )])
    }

    /// Multipliziert das Grid-Tempo — 0.5 und 2 räumen Oktavfehler auf.
    ///
    /// Der Anker bleibt: Bei einer Halbierung liegt jede zweite Eins weiterhin
    /// richtig, und beim Verdoppeln erst recht.
    fn grid_skalieren(&mut self, deck: usize, faktor: f64) -> Result<Vec<String>, Fehler> {
        if faktor <= 0.0 {
            return Err(Fehler::Argument {
                control: format!("deck{}.grid_scale", deck + 1),
                erwartet: "eine positive Zahl, meist 0.5 oder 2".into(),
            });
        }
        let Some(d) = self.decks.get(deck) else {
            return Err(Fehler::UnbekanntesControl(format!("deck{}", deck + 1)));
        };
        let Some(grid) = d.state.grid() else {
            return Err(Fehler::Gescheitert(
                "kein Grid zum Skalieren — erst bpm_grid setzen".into(),
            ));
        };

        let neu = grid.bpm as f64 * faktor;
        let begrenzt = neu.clamp(0.0, 400.0) as f32;
        d.state
            .set_grid(Some(Beatgrid::new(begrenzt, grid.anchor_frames, 1.0)));
        self.grid_sichern(deck);

        Ok(vec![format!(
            "grid deck{} {:.2} → {:.2} BPM",
            deck + 1,
            grid.bpm,
            begrenzt
        )])
    }

    /// Schreibt ein korrigiertes Beatgrid in die Sammlung zurück.
    ///
    /// Still, aus demselben Grund wie bei den Cues: Wer das Grid gerade zieht,
    /// hat das Grid gemeint. Was schiefging, steht in `load_status`.
    fn grid_sichern(&mut self, deck: usize) {
        let Some(d) = self.decks.get(deck) else {
            return;
        };
        if d.pfad.is_empty() {
            return;
        }
        let Some(grid) = d.state.grid() else { return };
        let Some(sammlung) = self.sammlung.as_ref() else {
            return;
        };

        let anker = grid.anchor_frames as f64 / d.sample_rate as f64;
        if let Err(e) = sammlung.grid_speichern(&d.pfad, grid.bpm, anker) {
            self.decks[deck].lade_status = format!("grid nicht gespeichert: {e}");
        }
    }

    /// Schreibt die Hot Cues eines Decks in die Sammlung zurück.
    ///
    /// Still: Wer einen Cue setzt, hat den Cue gemeint, nicht einen
    /// Datenbankvorgang. Scheitert das Schreiben, steht der Cue trotzdem am
    /// Deck und die laufende Nummer geht weiter — sichtbar wird es beim
    /// nächsten Laden, und dort ist es auch zu reparieren.
    fn cues_sichern(&mut self, deck: usize) {
        let Some(d) = self.decks.get(deck) else {
            return;
        };
        if d.pfad.is_empty() {
            return;
        }
        let Some(sammlung) = self.sammlung.as_ref() else {
            return;
        };

        let rate = d.sample_rate as f64;
        let cues: Vec<(usize, f64)> = (0..katalog::HOT_CUES)
            .filter_map(|c| d.state.cue(c).map(|f| (c, f as f64 / rate)))
            .collect();

        if let Err(e) = sammlung.hot_cues_speichern(&d.pfad, &cues) {
            self.decks[deck].lade_status = format!("cues nicht gespeichert: {e}");
        }
    }

    fn schreibe_kanal(
        &mut self,
        i: usize,
        b: &Beschreibung,
        k: &Schluessel,
        wert: Wert,
    ) -> Result<(), Fehler> {
        if i >= self.kanaele.len() {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
        }

        // Erst prüfen und umrechnen, dann spiegeln, dann senden. Scheitert die
        // Prüfung, ist der Spiegel unberührt und stimmt weiter mit dem Mixer
        // überein.
        let element = k.element.as_str();
        if element == "fx" {
            let name = wert.als_text().ok_or_else(|| Fehler::FalscherTyp {
                control: k.to_string(),
                erwartet: Art::Auswahl,
            })?;
            let effekt = Effekt::aus_name(name).ok_or_else(|| Fehler::UnbekannteAuswahl {
                control: k.to_string(),
                erlaubt: b.auswahl.iter().map(|s| s.to_string()).collect(),
            })?;
            self.kanaele[i].fx = effekt;
            self.handle.send(Command::Fx(i, effekt));
            return Ok(());
        }

        if element == "assign" {
            let name = wert.als_text().ok_or_else(|| Fehler::FalscherTyp {
                control: k.to_string(),
                erwartet: Art::Auswahl,
            })?;
            let assign = assign_aus_name(name).ok_or_else(|| Fehler::UnbekannteAuswahl {
                control: k.to_string(),
                erlaubt: b.auswahl.iter().map(|s| s.to_string()).collect(),
            })?;
            self.kanaele[i].assign = assign;
            self.handle.send(Command::Assign(i, assign));
            return Ok(());
        }

        if element == "cue" {
            let an = Self::schalter(b, k, &wert)?;
            self.kanaele[i].cue = an;
            self.handle.send(Command::Cue(i, an));
            return Ok(());
        }

        let v = Self::zahl(b, k, &wert)?;
        let kanal = &mut self.kanaele[i];

        let befehl = match element {
            "trim" => {
                kanal.trim = v;
                Command::Trim(i, v as f32)
            }
            "fader" => {
                kanal.fader = v;
                Command::Fader(i, v as f32)
            }
            "filter" => {
                kanal.filter = v;
                Command::Filter(i, v as f32)
            }
            "fx_mix" => {
                kanal.fx_mix = v;
                Command::FxMix(i, v as f32)
            }
            "fx_amount" => {
                kanal.fx_amount = v;
                Command::FxAmount(i, v as f32)
            }
            "fx_time" => {
                kanal.fx_time = v;
                Command::FxTime(i, v as f32)
            }
            "eq_low" | "eq_mid" | "eq_high" => {
                match element {
                    "eq_low" => kanal.eq_low = v,
                    "eq_mid" => kanal.eq_mid = v,
                    _ => kanal.eq_high = v,
                }
                // Der EQ wird als Ganzes geschickt — die drei Bänder hängen im
                // Mixer an einem Filtersatz.
                Command::Eq {
                    channel: i,
                    low: kanal.eq_low as f32,
                    mid: kanal.eq_mid as f32,
                    high: kanal.eq_high as f32,
                }
            }
            _ => return Err(Fehler::UnbekanntesControl(k.to_string())),
        };

        self.handle.send(befehl);
        Ok(())
    }

    fn schreibe_master(
        &mut self,
        b: &Beschreibung,
        k: &Schluessel,
        wert: Wert,
    ) -> Result<(), Fehler> {
        // Signale gehen nicht in den Mixer, sondern in die Ablage daneben.
        if let Some((i, feld)) = Self::signal_teile(&k.element) {
            return match feld {
                "name" => {
                    let Wert::Text(name) = wert else {
                        return Err(Fehler::FalscherTyp {
                            control: k.to_string(),
                            erwartet: crate::wert::Art::Text,
                        });
                    };
                    self.signale[i].name = name;
                    Ok(())
                }
                "wert" => {
                    let v = Self::zahl(b, k, &wert)?;
                    self.signale[i].setzen(b.begrenzen(v), std::time::Instant::now());
                    Ok(())
                }
                // Trend und Alter rechnen sich aus den Proben; sie zu setzen
                // hieße, die Vergangenheit zu behaupten.
                _ => Err(Fehler::NichtSchreibbar(k.to_string())),
            };
        }

        let v = Self::zahl(b, k, &wert)?;

        let befehl = match k.element.as_str() {
            "crossfader" => {
                self.master.crossfader = v;
                Command::Crossfader(v as f32)
            }
            "crossfader_curve" => {
                self.master.crossfader_curve = v;
                Command::CrossfaderCurve(v as f32)
            }
            "gain" => {
                self.master.gain = v;
                Command::MasterGain(v as f32)
            }
            "cue_gain" => {
                self.master.cue_gain = v;
                Command::CueGain(v as f32)
            }
            "cue_mix" => {
                self.master.cue_mix = v;
                Command::CueMix(v as f32)
            }
            _ => return Err(Fehler::UnbekanntesControl(k.to_string())),
        };

        self.handle.send(befehl);
        Ok(())
    }
}

/// Wie viele Treffer eine Suche höchstens zurückgibt.
pub const GRENZE: usize = 200;

fn treffer_zeilen(treffer: &[Treffer]) -> Vec<String> {
    let mut zeilen: Vec<String> = treffer
        .iter()
        .map(|t| {
            let bpm = match t.bpm {
                Some(b) => format!("{b:.2}"),
                None => "-".into(),
            };
            // Camelot, nicht der Name: Wer die Zeile liest, will mischen, und
            // dafür ist die Zahl auf dem Rad die brauchbare Form.
            let key = match t.tonart {
                Some(k) => k.camelot(),
                None => "-".into(),
            };
            format!("track {bpm} {key} {} {}", t.pfad, t.titel)
        })
        .collect();

    // Wenn die Grenze greift, muss das dastehen. Eine abgeschnittene Liste,
    // die wie eine vollständige aussieht, ist eine Falschaussage.
    if treffer.len() >= GRENZE {
        zeilen.push(format!("hinweis auf {GRENZE} Treffer begrenzt"));
    }
    zeilen
}

pub fn assign_name(assign: Assign) -> &'static str {
    match assign {
        Assign::A => "a",
        Assign::B => "b",
        Assign::Thru => "thru",
    }
}

pub fn assign_aus_name(name: &str) -> Option<Assign> {
    match name {
        "a" | "A" => Some(Assign::A),
        "b" | "B" => Some(Assign::B),
        "thru" | "Thru" => Some(Assign::Thru),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    /// Vergleich mit Toleranz.
    ///
    /// Tempo und Reglerwerte liegen im Deck und im Mixer als `f32` — dort
    /// gehört die Genauigkeit hin, denn sie werden pro Sample benutzt. Auf
    /// exakte Gleichheit zu prüfen hieße, eine Genauigkeit zu behaupten, die
    /// es nicht gibt: 1.04 als `f32` und zurück ist 1.0399999618530273.
    fn gleich(a: &Wert, b: &Wert) -> bool {
        match (a, b) {
            (Wert::Zahl(x), Wert::Zahl(y)) => (x - y).abs() < 1e-6,
            _ => a == b,
        }
    }

    #[test]
    fn geschriebene_werte_lassen_sich_wieder_lesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        for (control, wert) in [
            ("channel1.fader", Wert::Zahl(0.75)),
            ("channel1.eq_low", Wert::Zahl(0.0)),
            ("channel2.trim", Wert::Zahl(1.5)),
            ("master.crossfader", Wert::Zahl(-0.5)),
            ("master.gain", Wert::Zahl(0.9)),
            ("deck1.tempo", Wert::Zahl(1.04)),
            ("deck2.keylock", Wert::Schalter(true)),
            ("deck1.play", Wert::Schalter(true)),
        ] {
            pult.schreibe(&k(control), wert.clone()).expect(control);
            let gelesen = pult.lies(&k(control)).expect(control);
            assert!(
                gleich(&gelesen, &wert),
                "{control}: {gelesen:?} statt {wert:?}"
            );
        }
    }

    #[test]
    fn der_spiegel_bleibt_bei_einem_fehler_unberuehrt() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(0.6))
            .unwrap();

        // Ein Text auf einem Zahlen-Control muss scheitern …
        let fehler = pult.schreibe(&k("channel1.fader"), Wert::Text("laut".into()));
        assert!(fehler.is_err());

        // … und darf den vorherigen Wert nicht angetastet haben, sonst liefen
        // Spiegel und Mixer auseinander.
        assert_eq!(pult.lies(&k("channel1.fader")).unwrap(), Wert::Zahl(0.6));
    }

    #[test]
    fn werte_ausserhalb_des_bereichs_werden_begrenzt() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        pult.schreibe(&k("channel1.fader"), Wert::Zahl(9.0))
            .unwrap();
        assert_eq!(pult.lies(&k("channel1.fader")).unwrap(), Wert::Zahl(1.0));

        pult.schreibe(&k("deck1.tempo"), Wert::Zahl(3.0)).unwrap();
        assert!(gleich(
            &pult.lies(&k("deck1.tempo")).unwrap(),
            &Wert::Zahl(katalog::TEMPO_MAX)
        ));
    }

    #[test]
    fn nur_lesbare_controls_weisen_das_schreiben_ab() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.schreibe(&k("deck1.duration"), Wert::Zahl(10.0));
        assert_eq!(
            fehler,
            Err(Fehler::NichtSchreibbar("deck1.duration".into()))
        );
    }

    #[test]
    fn unbekannte_controls_werden_benannt_abgewiesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        assert_eq!(
            pult.lies(&k("deck1.quatsch")),
            Err(Fehler::UnbekanntesControl("deck1.quatsch".into()))
        );
        // Ein Deck, das es nicht gibt, ist kein stiller Fehlschlag.
        assert_eq!(
            pult.lies(&k("deck9.play")),
            Err(Fehler::UnbekanntesControl("deck9.play".into()))
        );
        assert!(pult
            .schreibe(&k("channel9.fader"), Wert::Zahl(1.0))
            .is_err());
    }

    #[test]
    fn die_auswahl_nimmt_nur_ihre_eigenen_namen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        pult.schreibe(&k("channel1.assign"), Wert::Auswahl("thru".into()))
            .unwrap();
        assert_eq!(
            pult.lies(&k("channel1.assign")).unwrap(),
            Wert::Auswahl("thru".into())
        );

        let fehler = pult.schreibe(&k("channel1.assign"), Wert::Auswahl("mitte".into()));
        assert!(matches!(fehler, Err(Fehler::UnbekannteAuswahl { .. })));
    }

    #[test]
    fn normiertes_schreiben_trifft_die_enden_und_die_mitte() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        // Ein MIDI-Regler in der Raste steht bei einem bipolaren Control auf 0.
        pult.schreibe_normiert(&k("channel1.filter"), 0.5).unwrap();
        assert_eq!(pult.lies(&k("channel1.filter")).unwrap(), Wert::Zahl(0.0));

        pult.schreibe_normiert(&k("channel1.filter"), 1.0).unwrap();
        assert_eq!(pult.lies(&k("channel1.filter")).unwrap(), Wert::Zahl(1.0));

        // Auf einem Schalter ist die obere Hälfte „an".
        pult.schreibe_normiert(&k("deck1.play"), 1.0).unwrap();
        assert_eq!(pult.lies(&k("deck1.play")).unwrap(), Wert::Schalter(true));
        pult.schreibe_normiert(&k("deck1.play"), 0.0).unwrap();
        assert_eq!(pult.lies(&k("deck1.play")).unwrap(), Wert::Schalter(false));
    }

    /// Ein gesetzter Cue muss den Trackwechsel überleben.
    ///
    /// Vorher lag er nur in den Atomics des Decks: acht Cues gesetzt, Track neu
    /// geladen, weg. Und die Sammlung hatte die Tabelle die ganze Zeit.
    #[test]
    fn ein_gesetzter_cue_landet_in_der_sammlung() {
        let (mut pult, _runner, protokoll, _) = crate::testing::pult_mit_protokoll();
        pult.deck_mut(0).unwrap().pfad = "/musik/a.wav".into();

        pult.schreibe(&k("deck1.cue2"), Wert::Zahl(10.0)).unwrap();
        pult.schreibe(&k("deck1.cue5"), Wert::Zahl(30.5)).unwrap();

        let gesichert = protokoll.lock().unwrap()["/musik/a.wav"].clone();
        assert_eq!(gesichert, vec![(1, 10.0), (4, 30.5)], "{gesichert:?}");

        // Löschen wird genauso weitergereicht, sonst käme ein gelöschter Cue
        // beim nächsten Laden wieder.
        pult.schreibe(&k("deck1.cue2"), Wert::Leer).unwrap();
        assert_eq!(
            protokoll.lock().unwrap()["/musik/a.wav"],
            vec![(4, 30.5)],
            "ein gelöschter Cue käme sonst beim nächsten Laden wieder"
        );
    }

    /// Ohne geladenen Track gibt es nichts zurückzuschreiben.
    #[test]
    fn ein_leeres_deck_schreibt_keine_cues() {
        let (mut pult, _runner, protokoll, _) = crate::testing::pult_mit_protokoll();
        pult.schreibe(&k("deck1.cue1"), Wert::Zahl(5.0)).unwrap();
        assert!(protokoll.lock().unwrap().is_empty());
    }

    /// Ein fehlgeschlagenes Speichern darf den Cue nicht verschlucken — und
    /// nicht stillschweigend durchgehen.
    #[test]
    fn ein_gescheitertes_speichern_steht_im_status() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.deck_mut(0).unwrap().pfad = "/musik/a.schreibgeschuetzt".into();

        pult.schreibe(&k("deck1.cue1"), Wert::Zahl(5.0)).unwrap();

        // Der Cue steht am Deck …
        assert_eq!(pult.lies(&k("deck1.cue1")).unwrap(), Wert::Zahl(5.0));
        // … und dass er nicht in der Sammlung ankam, ist ablesbar.
        let Wert::Text(status) = pult.lies(&k("deck1.load_status")).unwrap() else {
            panic!("load_status ist kein Text");
        };
        assert!(status.contains("cues nicht gespeichert"), "{status}");
    }

    #[test]
    fn hot_cues_lassen_sich_setzen_und_wieder_leeren() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        assert_eq!(pult.lies(&k("deck1.cue3")).unwrap(), Wert::Leer);

        pult.schreibe(&k("deck1.cue3"), Wert::Zahl(12.5)).unwrap();
        assert_eq!(pult.lies(&k("deck1.cue3")).unwrap(), Wert::Zahl(12.5));

        pult.schreibe(&k("deck1.cue3"), Wert::Leer).unwrap();
        assert_eq!(pult.lies(&k("deck1.cue3")).unwrap(), Wert::Leer);

        // Über die Anzahl hinaus gibt es keine.
        assert!(pult.lies(&k("deck1.cue9")).is_err());
    }

    #[test]
    fn die_liste_waechst_mit_den_angemeldeten_geraeten() {
        let (pult, _runner) = pult_mit_zwei_decks();
        let liste = pult.liste();

        // Zwei Decks, drei Kanäle, ein Master — nichts Erfundenes.
        assert!(liste.iter().any(|(k, _)| k.to_string() == "deck2.play"));
        assert!(liste.iter().any(|(k, _)| k.to_string() == "channel3.fader"));
        assert!(!liste.iter().any(|(k, _)| k.to_string() == "deck3.play"));
        assert!(!liste.iter().any(|(k, _)| k.to_string() == "channel4.fader"));

        // Und jedes aufgezählte Control lässt sich auch wirklich lesen —
        // Aktionen ausgenommen, die haben keinen Zustand.
        for (schluessel, b) in &liste {
            if b.art == Art::Aktion {
                continue;
            }
            pult.lies(schluessel)
                .unwrap_or_else(|e| panic!("{schluessel} steht in der Liste, aber: {e}"));
        }
    }

    #[test]
    fn jedes_schreibbare_control_laesst_sich_auch_schreiben() {
        // Der Katalog verspricht Schreibbarkeit — das hier prüft, dass die
        // Umsetzung dahinter existiert und nicht nur die Beschreibung.
        let (mut pult, _runner) = pult_mit_zwei_decks();

        for (schluessel, b) in pult.liste() {
            if !b.schreibbar || b.art == Art::Aktion {
                continue;
            }
            let wert = match b.art {
                Art::Schalter => Wert::Schalter(true),
                Art::Auswahl => Wert::Auswahl(b.auswahl[0].to_string()),
                // Ein Textfeld nimmt keine Zahl: Der Typ ist die Zusage, und
                // sie stillschweigend zu dehnen hieße, sie aufzugeben.
                Art::Text => Wert::Text("Probe".into()),
                _ => Wert::Zahl(b.aus_normiert(0.5)),
            };
            pult.schreibe(&schluessel, wert)
                .unwrap_or_else(|e| panic!("{schluessel} ist als schreibbar gemeldet, aber: {e}"));
        }
    }

    #[test]
    fn jede_aufgezaehlte_aktion_ist_auch_umgesetzt() {
        // Die Gegenprobe zur Liste: Eine Aktion, die im Katalog steht, aber
        // im Pult nicht verdrahtet ist, würde sich mit „unbekanntes Control"
        // melden — und das wäre eine Lüge, denn aufgezählt hat sie sich.
        let (mut pult, _runner) = pult_mit_zwei_decks();

        for (schluessel, b) in pult.liste() {
            if b.art != Art::Aktion {
                continue;
            }
            // Mit einem plausiblen Argument, damit nicht schon daran scheitert.
            let argument = match schluessel.element.as_str() {
                "load" => Some("/musik/track.wav"),
                "jump_cue" => Some("1"),
                "beatjump" => Some("4"),
                "search" => Some("test"),
                "search_mixable" => Some("128"),
                "search_harmonic" => Some("Am"),
                "grid_scale" => Some("2"),
                "playlist" => Some("Freitag"),
                "fx_sync" => Some("1"),
                "record" => Some("/tmp/musik-katalogtest.wav"),
                "queue_add" => Some("/musik/vorgemerkt.wav"),
                "queue_note" => Some("1 passt harmonisch"),
                "queue_bump" | "queue_drop" => Some("1"),
                _ => None,
            };

            match pult.ausloesen(&schluessel, argument) {
                Ok(_) => {}
                // Ein sachlicher Fehlschlag ist in Ordnung — ein nicht
                // gesetzter Hot Cue etwa. Ein unbekanntes Control nicht.
                Err(Fehler::Gescheitert(_)) => {}
                Err(e) => panic!("{schluessel} steht im Katalog, aber: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod aktions_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    #[test]
    fn sync_zieht_das_deck_auf_das_andere() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.decks()[0]
            .state
            .set_grid(Some(audio_core::Beatgrid::new(128.0, 0, 1.0)));
        pult.decks()[1]
            .state
            .set_grid(Some(audio_core::Beatgrid::new(124.0, 0, 1.0)));

        let zeilen = pult.ausloesen(&k("deck2.sync"), None).expect("sync");
        assert!(zeilen[0].starts_with("sync deck2 auf deck1"), "{zeilen:?}");

        // Das Tempo muss wirklich angekommen sein: 128/124 ≈ 1.0323.
        let Wert::Zahl(tempo) = pult.lies(&k("deck2.tempo")).unwrap() else {
            panic!("Tempo ist keine Zahl");
        };
        assert!((tempo - 128.0 / 124.0).abs() < 1e-3, "Tempo {tempo}");
    }

    #[test]
    fn sync_ohne_beatgrid_sagt_das_statt_zu_raten() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.decks()[1].state.set_grid(None);

        let fehler = pult.ausloesen(&k("deck2.sync"), None);
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
    }

    #[test]
    fn ein_deck_laesst_sich_nicht_auf_sich_selbst_ziehen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("deck1.sync"), Some("deck1"));
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
    }

    #[test]
    fn hot_cues_lassen_sich_ausloesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("deck1.cue2"), Wert::Zahl(10.0)).unwrap();

        // Der Sprung selbst ist erst ein Wunsch, den der Audio-Thread
        // ausführt — dass er ankommt, prüfen die Tests in `audio-core`. Hier
        // zählt, dass die Aktion überhaupt durchgereicht wird.
        assert!(pult.ausloesen(&k("deck1.jump_cue"), Some("2")).is_ok());
    }

    #[test]
    fn ein_ungesetzter_hot_cue_springt_nirgendwohin() {
        // Stumm an den Anfang zu springen wäre auf einer Anlage ein Unfall.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("deck1.jump_cue"), Some("5"));
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");

        let daneben = pult.ausloesen(&k("deck1.jump_cue"), Some("99"));
        assert!(
            matches!(daneben, Err(Fehler::Argument { .. })),
            "{daneben:?}"
        );
    }

    /// Der häufigste Fall: ein Oktavfehler des Detektors.
    ///
    /// 180 statt 90 ist kein Randfall — bei einem der fünf Testtracks lag die
    /// Tempo-Konfidenz bei 0,05, und ohne Korrekturmöglichkeit wäre er
    /// unbrauchbar.
    #[test]
    fn ein_oktavfehler_laesst_sich_halbieren() {
        let (mut pult, _runner, _, grid) = crate::testing::pult_mit_protokoll();
        pult.deck_mut(0).unwrap().pfad = "/musik/a.wav".into();
        pult.decks()[0]
            .state
            .set_grid(Some(audio_core::Beatgrid::new(180.0, 4_800, 1.0)));

        let zeilen = pult.ausloesen(&k("deck1.grid_scale"), Some("0.5")).unwrap();
        assert!(zeilen[0].contains("180.00 → 90.00"), "{zeilen:?}");
        assert_eq!(pult.lies(&k("deck1.bpm_grid")).unwrap(), Wert::Zahl(90.0));

        // Der Anker bleibt stehen — jede zweite Eins lag ja richtig.
        let Wert::Zahl(anker) = pult.lies(&k("deck1.grid_anchor")).unwrap() else {
            panic!("kein Anker");
        };
        assert!((anker - 0.1).abs() < 1e-6, "Anker {anker}");

        // Und die Korrektur überlebt den Trackwechsel.
        let (pfad, bpm, sekunden) = grid.lock().unwrap().clone().expect("nichts gespeichert");
        assert_eq!(pfad, "/musik/a.wav");
        assert_eq!(bpm, 90.0);
        assert!((sekunden - 0.1).abs() < 1e-6, "{sekunden}");
    }

    /// „Der Beat ist hier" — der Handgriff, wenn die Eins verschoben liegt.
    #[test]
    fn der_anker_laesst_sich_auf_die_position_legen() {
        let (mut pult, _runner, _, grid) = crate::testing::pult_mit_protokoll();
        pult.deck_mut(0).unwrap().pfad = "/musik/a.wav".into();
        pult.schreibe(&k("deck1.position"), Wert::Zahl(12.5))
            .unwrap();

        let zeilen = pult.ausloesen(&k("deck1.grid_here"), None).unwrap();
        assert!(zeilen[0].contains("12.500 s"), "{zeilen:?}");

        let Wert::Zahl(anker) = pult.lies(&k("deck1.grid_anchor")).unwrap() else {
            panic!("kein Anker");
        };
        assert!((anker - 12.5).abs() < 1e-3, "Anker {anker}");

        // Das Tempo hat sich dabei nicht verändert — verschoben wurde die Phase.
        assert_eq!(pult.lies(&k("deck1.bpm_grid")).unwrap(), Wert::Zahl(128.0));
        let (_, bpm, sekunden) = grid.lock().unwrap().clone().expect("nichts gespeichert");
        assert_eq!(bpm, 128.0);
        assert!((sekunden - 12.5).abs() < 1e-3);
    }

    /// Ohne Grid gibt es nichts zu korrigieren — und nichts zu raten.
    #[test]
    fn ohne_grid_wird_keine_korrektur_erfunden() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.decks()[0].state.set_grid(None);

        for (control, argument) in [("deck1.grid_here", None), ("deck1.grid_scale", Some("2"))] {
            let fehler = pult.ausloesen(&k(control), argument);
            assert!(
                matches!(fehler, Err(Fehler::Gescheitert(_))),
                "{control}: {fehler:?}"
            );
        }

        let anker = pult.schreibe(&k("deck1.grid_anchor"), Wert::Zahl(5.0));
        assert!(matches!(anker, Err(Fehler::Gescheitert(_))), "{anker:?}");

        // Das Tempo dagegen lässt sich auch aus dem Nichts setzen — sonst käme
        // man aus einem fehlenden Grid nie wieder heraus.
        pult.schreibe(&k("deck1.bpm_grid"), Wert::Zahl(174.0))
            .unwrap();
        assert_eq!(pult.lies(&k("deck1.bpm_grid")).unwrap(), Wert::Zahl(174.0));
    }

    #[test]
    fn beatjump_braucht_ein_grid() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.ausloesen(&k("deck1.beatjump"), Some("4")).unwrap();

        pult.decks()[0].state.set_grid(None);
        let fehler = pult.ausloesen(&k("deck1.beatjump"), Some("4"));
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
    }

    #[test]
    fn laden_meldet_sich_als_angenommen_und_nicht_als_fertig() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        let zeilen = pult
            .ausloesen(&k("deck1.load"), Some("/musik/track.wav"))
            .unwrap();
        assert_eq!(zeilen, vec!["load deck1 angenommen"]);
        assert_eq!(
            pult.lies(&k("deck1.load_status")).unwrap(),
            Wert::Text("laedt".into())
        );
    }

    #[test]
    fn ein_abgelehnter_ladeauftrag_faelscht_den_status_nicht() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("deck1.load"), Some("/musik/liste.txt"));

        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
        assert_eq!(
            pult.lies(&k("deck1.load_status")).unwrap(),
            Wert::Text("bereit".into()),
            "ein abgelehnter Auftrag darf nicht als laufend dastehen"
        );
    }

    #[test]
    fn laden_ohne_pfad_wird_abgewiesen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("deck1.load"), None);
        assert!(matches!(fehler, Err(Fehler::Argument { .. })), "{fehler:?}");
    }

    #[test]
    fn die_sammlung_laesst_sich_durchsuchen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let zeilen = pult.ausloesen(&k("master.search"), Some("techno")).unwrap();
        assert_eq!(zeilen.len(), 3);
        assert!(
            zeilen[0].starts_with("track 128.00 8A /musik/techno-0.wav"),
            "{zeilen:?}"
        );
    }

    #[test]
    fn die_tonart_eines_decks_steht_in_beiden_schreibweisen() {
        let (pult, _runner) = pult_mit_zwei_decks();

        assert_eq!(pult.lies(&k("deck1.key")).unwrap(), Wert::Text("Am".into()));
        assert_eq!(
            pult.lies(&k("deck1.key_camelot")).unwrap(),
            Wert::Text("8A".into())
        );
    }

    #[test]
    fn ein_deck_ohne_tonart_erfindet_keine() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.deck_mut(0).unwrap().tonart = None;

        assert_eq!(pult.lies(&k("deck1.key")).unwrap(), Wert::Leer);
        assert_eq!(pult.lies(&k("deck1.key_camelot")).unwrap(), Wert::Leer);
    }

    #[test]
    fn harmonisch_suchen_nimmt_beide_schreibweisen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        // Die Test-Sammlung gibt die angefragte Tonart im Pfad zurück — so
        // lässt sich prüfen, dass `Am` und `8A` dieselbe Anfrage ergeben.
        for eingabe in ["Am", "8A", "  am  "] {
            let zeilen = pult
                .ausloesen(&k("master.search_harmonic"), Some(eingabe))
                .unwrap_or_else(|e| panic!("{eingabe}: {e}"));
            assert!(
                zeilen[0].contains("/musik/8A-0.wav"),
                "{eingabe}: {zeilen:?}"
            );
        }
    }

    #[test]
    fn eine_unlesbare_tonart_wird_gemeldet_statt_leer_zu_antworten() {
        // Ein Tippfehler darf nicht wie eine leere Sammlung aussehen.
        let (mut pult, _runner) = pult_mit_zwei_decks();

        let fehler = pult.ausloesen(&k("master.search_harmonic"), Some("H-Dur"));
        assert!(matches!(fehler, Err(Fehler::Argument { .. })), "{fehler:?}");

        let ohne = pult.ausloesen(&k("master.search_harmonic"), None);
        assert!(matches!(ohne, Err(Fehler::Argument { .. })), "{ohne:?}");
    }

    #[test]
    fn aktionen_und_werte_werden_nicht_verwechselt() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        // Eine Aktion hat keinen Zustand, den man lesen könnte.
        assert!(matches!(
            pult.lies(&k("deck1.sync")),
            Err(Fehler::IstEineAktion(_))
        ));
        assert!(matches!(
            pult.schreibe(&k("deck1.sync"), Wert::Schalter(true)),
            Err(Fehler::IstEineAktion(_))
        ));
        // Und umgekehrt.
        assert!(matches!(
            pult.ausloesen(&k("deck1.play"), None),
            Err(Fehler::IstKeineAktion(_))
        ));
    }

    #[test]
    fn das_ende_eines_tracks_ist_von_aussen_sichtbar() {
        // Ohne das erfährt ein Agent nie, wann er nachlegen muss.
        let (pult, _runner) = pult_mit_zwei_decks();
        assert_eq!(
            pult.lies(&k("deck1.finished")).unwrap(),
            Wert::Schalter(false)
        );
    }
}

#[cfg(test)]
mod fx_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    #[test]
    fn effekte_lassen_sich_beim_namen_nennen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        pult.schreibe(&k("channel1.fx"), Wert::Auswahl("delay".into()))
            .unwrap();
        assert_eq!(
            pult.lies(&k("channel1.fx")).unwrap(),
            Wert::Auswahl("delay".into())
        );

        let fehler = pult.schreibe(&k("channel1.fx"), Wert::Auswahl("hall".into()));
        assert!(
            matches!(fehler, Err(Fehler::UnbekannteAuswahl { .. })),
            "{fehler:?}"
        );
    }

    #[test]
    fn fx_sync_rechnet_beats_in_sekunden() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        // Die Testdecks laufen auf 128 BPM: ein Beat = 60/128 = 0,46875 s.
        let zeilen = pult.ausloesen(&k("channel1.fx_sync"), Some("1")).unwrap();
        assert!(zeilen[0].contains("128.00 BPM"), "{zeilen:?}");

        let Wert::Zahl(zeit) = pult.lies(&k("channel1.fx_time")).unwrap() else {
            panic!("fx_time ist keine Zahl");
        };
        assert!((zeit - 60.0 / 128.0).abs() < 1e-6, "{zeit}");

        // Ein Achtel ist halb so lang.
        pult.ausloesen(&k("channel1.fx_sync"), Some("0.5")).unwrap();
        let Wert::Zahl(halb) = pult.lies(&k("channel1.fx_time")).unwrap() else {
            panic!()
        };
        assert!((halb - 30.0 / 128.0).abs() < 1e-6, "{halb}");
    }

    #[test]
    fn ohne_deck_sagt_fx_sync_das_statt_zu_raten() {
        // Kanal 3 ist AUX — der hat keinen Abspieler und damit kein Tempo.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("channel3.fx_sync"), Some("1"));
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
    }

    #[test]
    fn ohne_beatgrid_wird_die_effektzeit_nicht_geraten() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.decks()[0].state.set_grid(None);

        let fehler = pult.ausloesen(&k("channel1.fx_sync"), Some("1"));
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
    }

    #[test]
    fn eine_effektzeit_von_null_beats_ergibt_keinen_sinn() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("channel1.fx_sync"), Some("0"));
        assert!(matches!(fehler, Err(Fehler::Argument { .. })), "{fehler:?}");
    }
}

#[cfg(test)]
mod aufnahme_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("musik-pult-{}-{name}.wav", std::process::id()));
        p
    }

    #[test]
    fn ein_mitschnitt_laesst_sich_von_aussen_starten_und_beenden() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let pfad = scratch("lauf");

        assert_eq!(
            pult.lies(&k("master.recording")).unwrap(),
            Wert::Schalter(false)
        );

        let zeilen = pult
            .ausloesen(&k("master.record"), Some(&pfad.to_string_lossy()))
            .expect("Start");
        assert!(zeilen[0].starts_with("record läuft nach"), "{zeilen:?}");
        assert_eq!(
            pult.lies(&k("master.recording")).unwrap(),
            Wert::Schalter(true)
        );

        let zeilen = pult.ausloesen(&k("master.record_stop"), None).unwrap();
        assert!(zeilen[0].starts_with("record "), "{zeilen:?}");
        assert_eq!(
            pult.lies(&k("master.recording")).unwrap(),
            Wert::Schalter(false)
        );

        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn stoppen_ohne_mitschnitt_sagt_das() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("master.record_stop"), None);
        assert!(matches!(fehler, Err(Fehler::Gescheitert(_))), "{fehler:?}");
    }

    #[test]
    fn ohne_pfad_wird_nicht_aufgenommen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let fehler = pult.ausloesen(&k("master.record"), None);
        assert!(matches!(fehler, Err(Fehler::Argument { .. })), "{fehler:?}");
    }

    #[test]
    fn verlorene_frames_sind_von_aussen_sichtbar() {
        // Der Zähler muss abfragbar sein, sonst merkt man Lücken erst beim
        // Anhören des fertigen Mitschnitts.
        let (pult, _runner) = pult_mit_zwei_decks();
        assert_eq!(
            pult.lies(&k("master.record_dropped")).unwrap(),
            Wert::Zahl(0.0)
        );
        assert_eq!(
            pult.lies(&k("master.record_seconds")).unwrap(),
            Wert::Zahl(0.0)
        );
    }
}

#[cfg(test)]
mod musikalische_groessen_tests {
    use super::*;
    use crate::testing::{pult_mit_zwei_decks, rendern, RATE};

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    fn zahl_von(pult: &Steuerpult, name: &str) -> f64 {
        match pult.lies(&k(name)) {
            Ok(Wert::Zahl(v)) => v,
            andere => panic!("{name} ist keine Zahl: {andere:?}"),
        }
    }

    /// Der Rest zählt Grid-Beats und lässt sich vom Pitchfader nicht bewegen.
    ///
    /// Ein Beat ist eine feste Zahl Quell-Frames. Schneller abgespielt vergeht
    /// er in weniger Zeit, aber es werden davon nicht mehr — wer bei 1.08 auf
    /// einmal mehr Beats übrig hätte, hätte Zeit mit Takten verwechselt.
    /// Entscheidend ist, dass `in` und `ramp` genauso rechnen: „noch 32 Beats"
    /// und „in 32 Beats" müssen dieselbe Strecke meinen.
    #[test]
    fn der_rest_zaehlt_grid_beats_und_nicht_zeit() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let vorher = zahl_von(&pult, "deck1.beats_left");
        // 60 s bei 128 BPM.
        assert!((vorher - 128.0).abs() < 0.5, "{vorher}");

        pult.schreibe(&k("deck1.tempo"), Wert::Zahl(1.08)).unwrap();
        let schneller = zahl_von(&pult, "deck1.beats_left");
        assert!(
            (schneller - vorher).abs() < 0.01,
            "der Tempo-Regler hat die Beats verschoben: {vorher:.1} → {schneller:.1}"
        );
    }

    #[test]
    fn der_rest_schrumpft_beim_abspielen_und_wird_nicht_negativ() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        let vorher = zahl_von(&pult, "deck1.beats_left");
        rendern(&mut runner, RATE as usize * 2);
        let nachher = zahl_von(&pult, "deck1.beats_left");
        assert!(nachher < vorher, "{vorher:.1} → {nachher:.1}");

        // Über das Ende hinaus läuft es nicht ins Negative. Genau null wird es
        // nicht: Der Abspieler steht auf dem letzten Frame, nicht dahinter.
        rendern(&mut runner, RATE as usize * 90);
        let am_ende = zahl_von(&pult, "deck1.beats_left");
        assert!((0.0..0.01).contains(&am_ende), "am Ende: {am_ende}");
    }

    /// Auf der Grenze sind es null Beats bis zur Grenze — nicht eine ganze
    /// Phrase. Sonst spränge die Zahl genau dort, wo man sie abliest.
    #[test]
    fn auf_der_phrasengrenze_ist_der_abstand_null() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        assert_eq!(zahl_von(&pult, "deck1.beat"), 0.0);
        assert_eq!(zahl_von(&pult, "deck1.beats_to_phrase"), 0.0);

        // Vier Beats weiter sind es noch zwölf bis zur nächsten Sechzehnergruppe.
        // Ein Sprung ist nur ein Wunsch, den der Audio-Thread im nächsten Block
        // ausführt, und ein stehendes Deck rendert keinen. Also laufen lassen —
        // und mit einer Toleranz messen, weil in demselben Block schon wieder
        // gespielt wird.
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();
        pult.ausloesen(&k("deck1.beatjump"), Some("4")).unwrap();
        rendern(&mut runner, 1024);

        let beat = zahl_von(&pult, "deck1.beat");
        assert!((beat - 4.0).abs() < 0.1, "beat={beat}");
        let bis = zahl_von(&pult, "deck1.beats_to_phrase");
        assert!((bis - 12.0).abs() < 0.1, "bis zur Phrase={bis}");
    }

    #[test]
    fn die_phrasenlaenge_laesst_sich_umstellen() {
        // Nicht jede Musik ist in Sechzehnergruppen gebaut, und zwei Decks
        // können unterschiedlich liegen.
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();
        pult.ausloesen(&k("deck1.beatjump"), Some("4")).unwrap();
        rendern(&mut runner, 1024);
        assert!((zahl_von(&pult, "deck1.beats_to_phrase") - 12.0).abs() < 0.1);

        pult.schreibe(&k("deck1.phrase_beats"), Wert::Zahl(8.0))
            .unwrap();
        assert!((zahl_von(&pult, "deck1.beats_to_phrase") - 4.0).abs() < 0.1);
        // Das andere Deck bleibt, wo es war.
        assert_eq!(zahl_von(&pult, "deck2.phrase_beats"), PHRASE_BEATS);
    }

    #[test]
    fn ohne_beatgrid_gibt_es_keine_beats() {
        // Leer und nicht null: Null hieße „gerade auf der Eins", und das wäre
        // eine Behauptung über einen Track, von dem niemand das Raster kennt.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("deck1.bpm_grid"), Wert::Zahl(0.0)).ok();

        for name in ["deck1.beat", "deck1.beats_left", "deck1.beats_to_phrase"] {
            assert_eq!(pult.lies(&k(name)).unwrap(), Wert::Leer, "{name}");
        }
    }
}

#[cfg(test)]
mod signal_tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    /// Der ganze Zweck: Ein Signal von außen wird zu einem Control wie jedes
    /// andere — und damit lassen sich `when`, `sub` und `ramp` darauf anwenden,
    /// ohne dass hier eine Zeile dafür geschrieben werden müsste.
    #[test]
    fn ein_signal_von_aussen_ist_ein_control_wie_jedes_andere() {
        let (mut pult, _runner) = pult_mit_zwei_decks();

        pult.schreibe(&k("master.signal1_name"), Wert::Text("Energie".into()))
            .unwrap();
        pult.schreibe(&k("master.signal1"), Wert::Zahl(0.4))
            .unwrap();

        assert_eq!(
            pult.lies(&k("master.signal1_name")).unwrap(),
            Wert::Text("Energie".into())
        );
        assert_eq!(pult.lies(&k("master.signal1")).unwrap(), Wert::Zahl(0.4));
        // Und es lässt sich als Bedingung verwenden — ohne Sonderweg.
        let mut plan = crate::Zeitplan::neu();
        crate::zeitplan::wenn_planen(
            &pult,
            &mut plan,
            k("master.signal1"),
            crate::zeitplan::Vergleich::Unter,
            0.2,
            "do master.queue_next".into(),
        )
        .expect("ein Signal muss sich als Bedingung eignen");
    }

    /// Solange nichts gemeldet wurde, ist der Wert leer — und nicht null.
    ///
    /// Null wäre eine Aussage über den Raum („keine Energie"), die niemand
    /// getroffen hat.
    #[test]
    fn ein_ungenutztes_signal_behauptet_nichts() {
        let (pult, _runner) = pult_mit_zwei_decks();
        for feld in ["", "_trend", "_age"] {
            assert_eq!(
                pult.lies(&k(&format!("master.signal2{feld}"))).unwrap(),
                Wert::Leer,
                "signal2{feld}"
            );
        }
        assert_eq!(
            pult.lies(&k("master.signal2_name")).unwrap(),
            Wert::Text(String::new())
        );
    }

    #[test]
    fn ein_trend_entsteht_erst_aus_mehreren_meldungen() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("master.signal1"), Wert::Zahl(0.3))
            .unwrap();
        assert_eq!(pult.lies(&k("master.signal1_trend")).unwrap(), Wert::Leer);

        pult.schreibe(&k("master.signal1"), Wert::Zahl(0.9))
            .unwrap();
        // Zwei Meldungen fast gleichzeitig: eine Richtung gibt es, ihre Größe
        // ist bei diesem Abstand beliebig — geprüft wird deshalb nur, dass
        // überhaupt eine dasteht.
        assert!(matches!(
            pult.lies(&k("master.signal1_trend")).unwrap(),
            Wert::Zahl(_)
        ));
    }

    #[test]
    fn was_sich_ausrechnet_laesst_sich_nicht_setzen() {
        // Trend und Alter folgen aus den Proben. Sie zu setzen hieße, die
        // Vergangenheit zu behaupten.
        let (mut pult, _runner) = pult_mit_zwei_decks();
        assert!(pult
            .schreibe(&k("master.signal1_trend"), Wert::Zahl(1.0))
            .is_err());
        assert!(pult
            .schreibe(&k("master.signal1_age"), Wert::Zahl(0.0))
            .is_err());
    }

    #[test]
    fn es_gibt_genau_so_viele_signale_wie_angekuendigt() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        let letztes = format!("master.signal{}", crate::signal::SIGNALE);
        pult.schreibe(&k(&letztes), Wert::Zahl(0.5)).unwrap();

        // Eins darüber gibt es nicht — und das muss ein Fehler sein, kein
        // stillschweigend angelegter Platz.
        let zuviel = format!("master.signal{}", crate::signal::SIGNALE + 1);
        assert!(pult.lies(&k(&zuviel)).is_err(), "{zuviel} wurde angenommen");
        assert!(pult.schreibe(&k(&zuviel), Wert::Zahl(0.5)).is_err());
    }

    #[test]
    fn signale_stehen_im_katalog_mit_ihrer_bedeutung() {
        // Sonst wüsste ein Agent nicht, dass es sie gibt.
        let (pult, _runner) = pult_mit_zwei_decks();
        let namen: Vec<String> = pult
            .liste()
            .iter()
            .map(|(s, _)| s.to_string())
            .filter(|n| n.contains("signal"))
            .collect();
        assert_eq!(namen.len(), crate::signal::SIGNALE * 4, "{namen:?}");
        assert!(namen.contains(&"master.signal1_trend".to_string()));
    }
}
