//! Die Oberfläche.
//!
//! Aufbau von oben nach unten: zwei Decks nebeneinander, darunter der Mixer,
//! darunter die Sammlung. Das ist die Anordnung, die man von einem DJ-Setup
//! kennt — Plattenspieler oben, Mischpult in der Mitte, Plattenkiste unten.
//!
//! Die Oberfläche fasst den Mixer nie direkt an. Sie hält gespiegelte Werte für
//! die Darstellung und schickt jede Änderung als Kommando in die lock-freie
//! Schlange. Den Transport steuert sie über die Atomics im `DeckState`.

use std::path::PathBuf;
use std::sync::Arc;

use analysis::peaks::PeakLevel;
use audio_core::deck::{DeckState, HOT_CUES};
use audio_engine::{Assign, Command, EngineHandle, Output};
use egui::{Color32, RichText, Ui};
use library::{Library, Query, TrackRecord};

use crate::theme;
use crate::waveform;

/// Breite eines Kanalzuges. Am Gerät sind das gut fünf Zentimeter — schmal
/// genug, dass vier Züge nebeneinander passen, breit genug zum Greifen.
const KANALBREITE: f32 = 132.0;
/// Länge des Linefaders. Ein kurzer Fader lässt sich nicht sauber einblenden.
const FADERHOEHE: f32 = 96.0;
/// Höhe des Mixerfeldes. Muss den ganzen Kanalzug fassen — ist es zu wenig,
/// schneidet das Panel oben die Beschriftung und unten die Fader ab.
const MIXER_HOEHE: f32 = 262.0;
/// Anfangshöhe der Plattenkiste. Sie ist ziehbar, weil man beim Suchen mehr
/// Liste will und beim Mixen mehr Wellenform.
const SAMMLUNG_HOEHE: f32 = 190.0;

pub struct ChannelUi {
    pub name: String,
    pub channel: usize,
    pub trim: f32,
    pub low: f32,
    pub mid: f32,
    pub high: f32,
    pub filter: f32,
    pub fader: f32,
    pub cue: bool,
}

impl ChannelUi {
    pub fn new(name: impl Into<String>, channel: usize) -> Self {
        ChannelUi {
            name: name.into(),
            channel,
            trim: 1.0,
            low: 1.0,
            mid: 1.0,
            high: 1.0,
            filter: 0.0,
            fader: 0.0,
            cue: false,
        }
    }
}

pub struct DeckUi {
    pub name: String,
    pub state: Arc<DeckState>,
    pub artist: String,
    pub titel: String,
    pub peaks: Vec<PeakLevel>,
    pub frames: u64,
    pub sample_rate: u32,
    pub strip: ChannelUi,
    /// Sichtbarer Ausschnitt der Zoom-Ansicht, in Sekunden.
    pub zoom_secs: f32,
    pub loop_beats: f64,
}

impl DeckUi {
    pub fn position_secs(&self) -> f64 {
        self.state.position_frames() as f64 / self.sample_rate as f64
    }

    pub fn dauer_secs(&self) -> f64 {
        self.frames as f64 / self.sample_rate as f64
    }

    pub fn cues(&self) -> Vec<(usize, u64)> {
        (0..HOT_CUES)
            .filter_map(|i| self.state.cue(i).map(|f| (i, f)))
            .collect()
    }
}

pub struct Screenshot {
    pub pfad: PathBuf,
    pub warte_bilder: u32,
    pub angefordert: bool,
}

pub struct MusikApp {
    pub handle: EngineHandle,
    /// Hält den Audio-Stream am Leben. Fällt er weg, verstummt die Ausgabe —
    /// deshalb liegt er hier, auch wenn sonst niemand ihn anfasst.
    #[allow(dead_code)]
    pub output: Option<Output>,
    pub audio_hinweis: String,
    pub decks: Vec<DeckUi>,
    pub aux: ChannelUi,
    pub crossfader: f32,
    pub crossfader_kurve: f32,
    pub master_gain: f32,
    pub cue_mix: f32,
    pub cue_gain: f32,
    pub library: Option<Library>,
    pub analyse_cache: PathBuf,
    pub suche: String,
    pub treffer: Vec<TrackRecord>,
    pub status: String,
    pub screenshot: Option<Screenshot>,
}

impl eframe::App for MusikApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Abgelöste Quellen abholen, sonst nimmt der Audio-Thread irgendwann
        // keine neuen Tracks mehr an.
        self.handle.collect_retired();

        // Kopie, weil `ui` gleich selbst ausgeliehen wird und der Kontext
        // danach noch für den Screenshot gebraucht wird.
        let ctx = ui.ctx().clone();

        // Die Decks laufen weiter, also muss auch weiter gezeichnet werden.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        egui::Panel::top("kopf").show(ui, |ui| self.kopfzeile(ui));

        // Die Sammlung wird zuerst angemeldet und liegt damit ganz unten; der
        // Mixer schiebt sich darüber, direkt unter die Decks. Genau die
        // Reihenfolge, in der man vor dem Gerät sitzt.
        egui::Panel::bottom("sammlung")
            .resizable(true)
            .default_size(SAMMLUNG_HOEHE)
            .show(ui, |ui| self.sammlung(ui));
        egui::Panel::bottom("mixer")
            .exact_size(MIXER_HOEHE)
            .show(ui, |ui| self.mixer(ui));

        egui::CentralPanel::default().show(ui, |ui| {
            // `columns` gibt jeder Spalte ein eigenes Top-down-Layout. Ein
            // `horizontal` täte das nicht — die Decks lägen dann seitwärts.
            let anzahl = self.decks.len().max(1);
            ui.columns(anzahl, |spalten| {
                for (index, spalte) in spalten.iter_mut().enumerate() {
                    self.deck(spalte, index);
                }
            });
        });

        self.screenshot_schritt(&ctx);
    }
}

impl MusikApp {
    fn kopfzeile(&mut self, ui: &mut Ui) {
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("MUSIK").strong().size(15.0));
            ui.label(
                RichText::new(&self.audio_hinweis)
                    .color(theme::TEXT_LEISE)
                    .size(11.0),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.status.is_empty() {
                    ui.label(
                        RichText::new(&self.status)
                            .color(theme::TEXT_LEISE)
                            .size(11.0),
                    );
                }
            });
        });
        ui.add_space(3.0);
    }

    fn deck(&mut self, ui: &mut Ui, index: usize) {
        let farbe = theme::deck_farbe(index);

        // Was nach Kopfzeilen, Transport und Hot Cues übrig bleibt, bekommen
        // die Wellenformen. Sonst klebt das Deck oben und darunter ist Leere.
        const RANDRUM: f32 = 178.0;
        let rest = (ui.available_height() - RANDRUM).max(120.0);
        let uebersicht = (rest * 0.26).clamp(38.0, 90.0);
        let zoom = rest - uebersicht;

        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(8.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                // Der Rahmen erbt das Layout des Elternteils; im Mixer wäre das
                // waagerecht. Explizit senkrecht, damit die Zeilen untereinander
                // stehen und nicht nebeneinander.
                ui.vertical(|ui| self.deck_inhalt(ui, index, farbe, uebersicht, zoom));
            });
    }

    fn deck_inhalt(
        &mut self,
        ui: &mut Ui,
        index: usize,
        farbe: Color32,
        uebersicht: f32,
        zoom: f32,
    ) {
        let (titel, artist, dauer, position, bpm, keylock, laeuft) = {
            let deck = &self.decks[index];
            (
                deck.titel.clone(),
                deck.artist.clone(),
                deck.dauer_secs(),
                deck.position_secs(),
                deck.state.effective_bpm(),
                deck.state.keylock(),
                deck.state.is_playing(),
            )
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.decks[index].name).color(farbe).strong());
            ui.label(RichText::new(titel).strong());
            ui.label(RichText::new(artist).color(theme::TEXT_LEISE).size(11.0));
        });

        ui.horizontal(|ui| {
            let tempo = match bpm {
                Some(b) => format!("{b:6.2} BPM"),
                None => "   — BPM".into(),
            };
            ui.label(
                RichText::new(tempo)
                    .monospace()
                    .color(farbe)
                    .size(17.0)
                    .strong(),
            );
            ui.label(
                RichText::new(format!("{}  /  {}", zeit(position), zeit(dauer)))
                    .monospace()
                    .color(theme::TEXT_LEISE),
            );
        });

        ui.add_space(2.0);
        self.wellenformen(ui, index, farbe, uebersicht, zoom);
        ui.add_space(4.0);
        self.transport(ui, index, laeuft, keylock);
        ui.add_space(2.0);
        self.hot_cues(ui, index);
    }

    fn wellenformen(
        &mut self,
        ui: &mut Ui,
        index: usize,
        farbe: Color32,
        hoehe_uebersicht: f32,
        hoehe_zoom: f32,
    ) {
        let deck = &self.decks[index];
        let gesamt = deck.frames as f64;
        let pos = deck.state.position_frames() as f64;
        let grid = deck.state.grid();
        let rate = deck.sample_rate;
        let cues = deck.cues();
        let zoom_frames = (deck.zoom_secs as f64 * rate as f64).max(1.0);

        // Übersicht über den ganzen Track.
        let (rect, antwort) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), hoehe_uebersicht),
            egui::Sense::click(),
        );
        if !deck.peaks.is_empty() && gesamt > 0.0 {
            let stufe = waveform::passende_stufe(&deck.peaks, gesamt, rect.width());
            waveform::zeichne(ui.painter(), rect, stufe, 0.0, gesamt, farbe, Some(pos));
            waveform::zeichne_cues(ui.painter(), rect, &cues, 0.0, gesamt);
            let x = rect.left() + (pos / gesamt) as f32 * rect.width();
            waveform::zeichne_playhead(ui.painter(), rect, x);
        }

        // Klick in die Übersicht springt dorthin.
        if antwort.clicked() {
            if let Some(p) = antwort.interact_pointer_pos() {
                let t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                self.decks[index].state.seek_frames((t * gesamt) as u64);
            }
        }

        ui.add_space(3.0);

        // Ausschnitt um die Abspielposition.
        let deck = &self.decks[index];
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), hoehe_zoom),
            egui::Sense::hover(),
        );
        let von = pos - zoom_frames * 0.5;
        let bis = pos + zoom_frames * 0.5;

        if !deck.peaks.is_empty() {
            let stufe = waveform::passende_stufe(&deck.peaks, zoom_frames, rect.width());
            waveform::zeichne(ui.painter(), rect, stufe, von, bis, farbe, Some(pos));
            if let Some(g) = grid {
                waveform::zeichne_grid(ui.painter(), rect, &g, rate, von, bis, gesamt);
            }
            waveform::zeichne_cues(ui.painter(), rect, &cues, von, bis);
            waveform::zeichne_playhead(ui.painter(), rect, rect.center().x);
        }
    }

    fn transport(&mut self, ui: &mut Ui, index: usize, laeuft: bool, keylock: bool) {
        ui.horizontal(|ui| {
            let beschriftung = if laeuft { "⏸  Pause" } else { "▶  Play" };
            if ui.button(RichText::new(beschriftung).size(13.0)).clicked() {
                self.decks[index].state.toggle_playing();
            }

            if ui.button("⏮").on_hover_text("An den Anfang").clicked() {
                self.decks[index].state.seek_frames(0);
            }

            let mut kl = keylock;
            if ui.checkbox(&mut kl, "Keylock").changed() {
                self.decks[index].state.set_keylock(kl);
            }

            // Sync richtet sich immer am jeweils anderen Deck aus.
            let anderes = 1 - index.min(1);
            if self.decks.len() > 1 && ui.button("SYNC").clicked() {
                let rate = self.decks[index].sample_rate;
                let master = Arc::clone(&self.decks[anderes].state);
                let slave = Arc::clone(&self.decks[index].state);

                match audio_engine::sync(&master, &slave, rate) {
                    Some(plan) => {
                        self.status = format!(
                            "{} auf {} gezogen: Tempo ×{:.4}, Phase {:+.3} Beats",
                            self.decks[index].name,
                            self.decks[anderes].name,
                            plan.tempo,
                            plan.phase_error_beats
                        )
                    }
                    None => self.status = "Sync braucht auf beiden Decks ein Beatgrid".into(),
                }
            }
        });

        ui.horizontal(|ui| {
            let mut tempo = self.decks[index].state.tempo();
            let antwort = ui.add(
                egui::Slider::new(&mut tempo, 0.92..=1.08)
                    .custom_formatter(|v, _| format!("{:+.2} %", (v - 1.0) * 100.0))
                    .text("Tempo"),
            );
            if antwort.changed() {
                self.decks[index].state.set_tempo(tempo);
            }
            if antwort.double_clicked() {
                self.decks[index].state.set_tempo(1.0);
            }
        });
    }

    fn hot_cues(&mut self, ui: &mut Ui, index: usize) {
        ui.horizontal_wrapped(|ui| {
            for i in 0..HOT_CUES {
                let gesetzt = self.decks[index].state.cue(i).is_some();
                let farbe = if gesetzt {
                    theme::CUE
                } else {
                    theme::TEXT_LEISE
                };

                let knopf = ui.add(
                    egui::Button::new(RichText::new(format!("{}", i + 1)).color(farbe).monospace())
                        .min_size(egui::vec2(24.0, 20.0)),
                );

                if knopf.clicked() {
                    let deck = &self.decks[index];
                    if gesetzt {
                        deck.state.jump_to_cue(i);
                    } else {
                        let pos = deck.state.position_frames();
                        deck.state.set_cue(i, Some(pos));
                    }
                }
                if knopf.secondary_clicked() {
                    self.decks[index].state.set_cue(i, None);
                }
            }

            ui.separator();

            let mut beats = self.decks[index].loop_beats;
            egui::ComboBox::from_id_salt(("loop", index))
                .width(58.0)
                .selected_text(format!("{} Beat", kurz(beats)))
                .show_ui(ui, |ui| {
                    for wert in [1.0, 2.0, 4.0, 8.0, 16.0] {
                        ui.selectable_value(&mut beats, wert, kurz(wert));
                    }
                });
            self.decks[index].loop_beats = beats;

            let laeuft = self.decks[index].state.is_looping();
            let text = if laeuft { "LOOP AUS" } else { "LOOP" };
            if ui
                .add(egui::Button::new(text).fill(if laeuft {
                    theme::AKTIV
                } else {
                    theme::PANEL_HELL
                }))
                .clicked()
            {
                let deck = &self.decks[index];
                if laeuft {
                    deck.state.set_loop_active(false);
                } else if deck.state.set_loop_beats(beats, deck.sample_rate) {
                    deck.state.set_loop_active(true);
                } else {
                    self.status = "Beat-Loop braucht ein Beatgrid".into();
                }
            }
        });
    }

    fn mixer(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            for index in 0..self.decks.len() {
                let farbe = theme::deck_farbe(index);
                let mut strip =
                    std::mem::replace(&mut self.decks[index].strip, ChannelUi::new("", 0));
                self.kanalzug(ui, &mut strip, farbe);
                self.decks[index].strip = strip;
            }

            let mut aux = std::mem::replace(&mut self.aux, ChannelUi::new("", 0));
            self.kanalzug(ui, &mut aux, theme::AUX);
            self.aux = aux;

            ui.separator();
            self.summe(ui);
        });
    }

    fn kanalzug(&mut self, ui: &mut Ui, strip: &mut ChannelUi, farbe: Color32) {
        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(7.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                // Senkrecht, sonst stünden die Regler eines Zuges nebeneinander
                // statt übereinander — der Rahmen erbt hier ein waagerechtes
                // Layout vom Mixer.
                ui.vertical(|ui| self.kanalzug_inhalt(ui, strip, farbe));
            });
    }

    fn kanalzug_inhalt(&mut self, ui: &mut Ui, strip: &mut ChannelUi, farbe: Color32) {
        ui.set_width(KANALBREITE);
        ui.label(RichText::new(&strip.name).color(farbe).strong().size(12.0));

        // Die Drehregler eines echten Mixers sind schmal; hier sind es liegende
        // Schieber, also bekommen sie die Breite des Zuges und keine Zahl —
        // beim Auflegen liest man die ohnehin nicht.
        let regler =
            |ui: &mut Ui, wert: &mut f32, name: &str, spanne: std::ops::RangeInclusive<f32>| {
                ui.spacing_mut().slider_width = KANALBREITE - 34.0;
                ui.add(
                    egui::Slider::new(wert, spanne)
                        .show_value(false)
                        .text(RichText::new(name).size(10.0)),
                )
                .changed()
            };

        if regler(ui, &mut strip.trim, "TRIM", 0.0..=2.0) {
            self.handle.send(Command::Trim(strip.channel, strip.trim));
        }

        let mut eq_geaendert = regler(ui, &mut strip.high, "HI", 0.0..=2.0);
        eq_geaendert |= regler(ui, &mut strip.mid, "MID", 0.0..=2.0);
        eq_geaendert |= regler(ui, &mut strip.low, "LOW", 0.0..=2.0);
        if eq_geaendert {
            self.handle.send(Command::Eq {
                channel: strip.channel,
                low: strip.low,
                mid: strip.mid,
                high: strip.high,
            });
        }

        if regler(ui, &mut strip.filter, "FLT", -1.0..=1.0) {
            self.handle
                .send(Command::Filter(strip.channel, strip.filter));
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("CUE").size(10.0))
                            .fill(if strip.cue {
                                theme::CUE
                            } else {
                                theme::PANEL_HELL
                            })
                            .min_size(egui::vec2(42.0, 22.0)),
                    )
                    .clicked()
                {
                    strip.cue = !strip.cue;
                    self.handle.send(Command::Cue(strip.channel, strip.cue));
                }
            });

            // Der Linefader steht senkrecht wie am Gerät.
            ui.spacing_mut().slider_width = FADERHOEHE;
            if ui
                .add(
                    egui::Slider::new(&mut strip.fader, 0.0..=1.0)
                        .show_value(false)
                        .vertical(),
                )
                .changed()
            {
                self.handle.send(Command::Fader(strip.channel, strip.fader));
            }
        });
    }

    fn summe(&mut self, ui: &mut Ui) {
        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(7.0)
            .corner_radius(4.0)
            .show(ui, |ui| {
                ui.vertical(|ui| self.summe_inhalt(ui));
            });
    }

    fn summe_inhalt(&mut self, ui: &mut Ui) {
        ui.set_width(250.0);
        ui.spacing_mut().slider_width = 214.0;
        ui.label(RichText::new("SUMME").strong().size(12.0));

        if ui
            .add(
                egui::Slider::new(&mut self.crossfader, -1.0..=1.0)
                    .show_value(false)
                    .text(RichText::new("A « Crossfader » B").size(10.0)),
            )
            .changed()
        {
            self.handle.send(Command::Crossfader(self.crossfader));
        }

        if ui
            .add(
                egui::Slider::new(&mut self.crossfader_kurve, 0.0..=1.0)
                    .show_value(false)
                    .text(RichText::new("Kurve weich » hart").size(10.0)),
            )
            .changed()
        {
            self.handle
                .send(Command::CrossfaderCurve(self.crossfader_kurve));
        }

        ui.separator();

        if ui
            .add(
                egui::Slider::new(&mut self.master_gain, 0.0..=1.5)
                    .show_value(false)
                    .text(RichText::new("MASTER").size(10.0)),
            )
            .changed()
        {
            self.handle.send(Command::MasterGain(self.master_gain));
        }

        if ui
            .add(
                egui::Slider::new(&mut self.cue_gain, 0.0..=1.5)
                    .show_value(false)
                    .text(RichText::new("KOPFHÖRER").size(10.0)),
            )
            .changed()
        {
            self.handle.send(Command::CueGain(self.cue_gain));
        }

        if ui
            .add(
                egui::Slider::new(&mut self.cue_mix, 0.0..=1.0)
                    .show_value(false)
                    .text(RichText::new("CUE « Mix » MASTER").size(10.0)),
            )
            .changed()
        {
            self.handle.send(Command::CueMix(self.cue_mix));
        }
    }

    fn sammlung(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("SAMMLUNG").strong().size(12.0));

            let feld = ui.add(
                egui::TextEdit::singleline(&mut self.suche)
                    .hint_text("Titel, Künstler, Album …")
                    .desired_width(240.0),
            );
            if feld.changed() {
                self.suchen();
            }

            if ui.button("Mischbar zu A").clicked() {
                self.mischbar_suchen(0);
            }
            if ui.button("Mischbar zu B").clicked() {
                self.mischbar_suchen(1);
            }

            if self.library.is_none() {
                ui.label(
                    RichText::new("keine Sammlung geladen — mit --db öffnen")
                        .color(theme::WARNUNG)
                        .size(11.0),
                );
            }
        });

        ui.add_space(3.0);

        egui::ScrollArea::vertical()
            // Ohne das schrumpft die Plattenkiste bei leerer Liste auf eine
            // Zeile zusammen und das Panel klappt mit.
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Grid::new("trefferliste")
                    .num_columns(4)
                    .striped(true)
                    .min_col_width(70.0)
                    .show(ui, |ui| {
                        for spalte in ["BPM", "KÜNSTLER", "TITEL", "LADEN"] {
                            ui.label(
                                RichText::new(spalte)
                                    .color(theme::TEXT_LEISE)
                                    .size(10.0)
                                    .strong(),
                            );
                        }
                        ui.end_row();

                        for eintrag in self.treffer.clone() {
                            let bpm = eintrag
                                .bpm
                                .map(|b| format!("{b:6.2}"))
                                .unwrap_or_else(|| "     —".into());
                            ui.label(RichText::new(bpm).monospace());
                            ui.label(eintrag.artist.clone().unwrap_or_else(|| "—".into()));
                            ui.label(anzeigename(&eintrag));

                            // Nur „A" und „B": ein Pfeilzeichen wäre schöner,
                            // aber die mitgelieferte Schrift hat keins, und ein
                            // fehlender Glyph wird zum leeren Kästchen.
                            ui.horizontal(|ui| {
                                for (deck, name) in [(0usize, "A"), (1usize, "B")] {
                                    let farbe = theme::deck_farbe(deck);
                                    if ui
                                        .small_button(RichText::new(name).color(farbe).strong())
                                        .on_hover_text(format!("auf Deck {name} laden"))
                                        .clicked()
                                    {
                                        self.laden(deck, &eintrag);
                                    }
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
    }

    fn suchen(&mut self) {
        let Some(lib) = self.library.as_ref() else {
            return;
        };
        let mut query = Query::text(self.suche.clone());
        query.limit = Some(200);
        self.treffer = lib.search(&query).unwrap_or_default();
    }

    fn mischbar_suchen(&mut self, deck: usize) {
        let Some(lib) = self.library.as_ref() else {
            return;
        };
        let Some(bpm) = self.decks.get(deck).and_then(|d| d.state.effective_bpm()) else {
            self.status = "Das Deck hat kein Tempo".into();
            return;
        };

        let mut query = Query::mixable_with(bpm, 0.06);
        query.limit = Some(200);
        self.treffer = lib.search(&query).unwrap_or_default();
        self.status = format!("Mischbar mit {bpm:.2} BPM: {} Treffer", self.treffer.len());
    }

    fn laden(&mut self, deck: usize, eintrag: &TrackRecord) {
        match crate::laden::track_auf_deck(self, deck, eintrag) {
            Ok(()) => self.status = format!("{} → {}", anzeigename(eintrag), self.decks[deck].name),
            Err(e) => self.status = format!("Laden fehlgeschlagen: {e}"),
        }
    }

    /// Nimmt nach ein paar Bildern ein Abbild auf und beendet sich.
    fn screenshot_schritt(&mut self, ctx: &egui::Context) {
        let Some(auftrag) = self.screenshot.as_mut() else {
            return;
        };

        if !auftrag.angefordert {
            if auftrag.warte_bilder > 0 {
                auftrag.warte_bilder -= 1;
                ctx.request_repaint();
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            auftrag.angefordert = true;
            ctx.request_repaint();
            return;
        }

        let bild = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });

        if let Some(bild) = bild {
            let pfad = auftrag.pfad.clone();
            match speichern(&bild, &pfad) {
                Ok(()) => println!("Screenshot: {}", pfad.display()),
                Err(e) => eprintln!("Screenshot fehlgeschlagen: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

fn speichern(bild: &egui::ColorImage, pfad: &std::path::Path) -> anyhow::Result<()> {
    let [breite, hoehe] = bild.size;
    let mut roh = Vec::with_capacity(breite * hoehe * 4);
    for pixel in &bild.pixels {
        roh.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b(), pixel.a()]);
    }

    let puffer = image::RgbaImage::from_raw(breite as u32, hoehe as u32, roh)
        .ok_or_else(|| anyhow::anyhow!("Bildgröße passt nicht zu den Daten"))?;
    puffer.save(pfad)?;
    Ok(())
}

/// Zuweisung eines Decks am Crossfader nach Index.
pub fn assign_fuer(index: usize) -> Assign {
    match index {
        0 => Assign::A,
        1 => Assign::B,
        _ => Assign::Thru,
    }
}

/// Was in der Liste als Titel steht.
///
/// Ohne Tags — bei WAV-Dateien der Normalfall — bleibt der Dateiname. Der
/// ganze Pfad wäre in einer Trefferliste unbrauchbar: er ist zu lang, und der
/// unterscheidende Teil steht ganz hinten.
pub fn anzeigename(eintrag: &TrackRecord) -> String {
    if let Some(titel) = eintrag.title.as_ref().filter(|t| !t.trim().is_empty()) {
        return titel.clone();
    }

    std::path::Path::new(&eintrag.path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| eintrag.path.clone())
}

/// Zahl ohne überflüssige Nachkommastellen — "4" statt "4.00".
fn kurz(wert: f64) -> String {
    if wert.fract().abs() < 1e-9 {
        format!("{}", wert.round() as i64)
    } else {
        format!("{wert:.2}")
    }
}

fn zeit(secs: f64) -> String {
    let secs = secs.max(0.0);
    let m = (secs / 60.0).floor() as u64;
    format!("{m}:{:05.2}", secs - m as f64 * 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ganze_zahlen_werden_ohne_nachkomma_gezeigt() {
        assert_eq!(kurz(4.0), "4");
        assert_eq!(kurz(16.0), "16");
        assert_eq!(kurz(0.5), "0.50");
    }

    #[test]
    fn zeit_wird_als_minuten_und_sekunden_gezeigt() {
        assert_eq!(zeit(0.0), "0:00.00");
        assert_eq!(zeit(65.5), "1:05.50");
        assert_eq!(zeit(-3.0), "0:00.00");
    }

    #[test]
    fn decks_liegen_auf_den_crossfader_seiten() {
        assert_eq!(assign_fuer(0), Assign::A);
        assert_eq!(assign_fuer(1), Assign::B);
        // Alles Weitere — AUX etwa — bleibt unberührt vom Crossfader.
        assert_eq!(assign_fuer(2), Assign::Thru);
    }

    #[test]
    fn ein_neuer_kanalzug_startet_mit_geschlossenem_fader() {
        let strip = ChannelUi::new("A", 0);
        assert_eq!(strip.fader, 0.0);
        assert!(!strip.cue);
        assert_eq!((strip.low, strip.mid, strip.high), (1.0, 1.0, 1.0));
        assert_eq!(strip.filter, 0.0);
    }
}
