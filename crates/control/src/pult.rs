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
use audio_engine::{Assign, Command, EngineHandle};

use crate::katalog::{self, Beschreibung};
use crate::schluessel::{Gruppe, Schluessel};
use crate::wert::{Art, Wert};

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
    decks: Vec<DeckEintrag>,
    kanaele: Vec<KanalSpiegel>,
    master: MasterSpiegel,
    handle: EngineHandle,
}

impl Steuerpult {
    pub fn neu(handle: EngineHandle) -> Steuerpult {
        Steuerpult {
            decks: Vec::new(),
            kanaele: Vec::new(),
            master: MasterSpiegel::default(),
            handle,
        }
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
        if self.beschreibung(k).is_none() {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
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
            "tempo" => Wert::Zahl(d.state.tempo() as f64),
            "keylock" => Wert::Schalter(d.state.keylock()),
            "beat_phase" => match d.state.beat_phase(d.sample_rate) {
                Some(p) => Wert::Zahl(p),
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
            _ => return None,
        };
        Some(wert)
    }

    fn lies_master(&self, element: &str) -> Option<Wert> {
        let m = &self.master;
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

    pub fn schreibe(&mut self, k: &Schluessel, wert: Wert) -> Result<(), Fehler> {
        let Some(b) = self.beschreibung(k) else {
            return Err(Fehler::UnbekanntesControl(k.to_string()));
        };
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
            }
        }

        Ok(())
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

        // Und jedes aufgezählte Control lässt sich auch wirklich lesen.
        for (schluessel, _) in &liste {
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
            if !b.schreibbar {
                continue;
            }
            let wert = match b.art {
                Art::Schalter => Wert::Schalter(true),
                Art::Auswahl => Wert::Auswahl(b.auswahl[0].to_string()),
                _ => Wert::Zahl(b.aus_normiert(0.5)),
            };
            pult.schreibe(&schluessel, wert)
                .unwrap_or_else(|e| panic!("{schluessel} ist als schreibbar gemeldet, aber: {e}"));
        }
    }
}
