//! Die Oberfläche.
//!
//! Aufbau von oben nach unten: zwei Decks nebeneinander, darunter der Mixer,
//! darunter die Sammlung. Das ist die Anordnung, die man von einem DJ-Setup
//! kennt — Plattenspieler oben, Mischpult in der Mitte, Plattenkiste unten.
//!
//! Die Oberfläche fasst den Mixer nie direkt an — und sie hält auch keine
//! eigenen Werte mehr. Alles Bedienbare liegt im Steuerpult (`control`), das
//! die Kommandos in die lock-freie Schlange schickt. Damit ist die Oberfläche
//! einer von mehreren Bedienern: Ein Skript am Socket bewegt denselben Fader,
//! und beide sehen sofort dasselbe.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use analysis::peaks::PeakLevel;
use audio_core::deck::{DeckState, HOT_CUES};
use audio_engine::{Assign, Output};
use control::{Einheit, Gruppe, Schluessel, Steuerpult, Treffer, Wert};
use egui::{Color32, RichText, Ui};

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

/// Was die Oberfläche über ein Deck weiß, das im Steuerpult nicht steht.
///
/// Titel, Tempo, Position und Reglerstellungen liegen im Pult — hier bleibt
/// nur, was allein zum Zeichnen gebraucht wird.
pub struct DeckUi {
    pub name: String,
    pub state: Arc<DeckState>,
    pub peaks: Vec<PeakLevel>,
    pub frames: u64,
    pub sample_rate: u32,
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
    /// Erst ab diesem Zeitpunkt wird ausgelöst.
    ///
    /// Eine feste Zahl von Bildern wäre willkürlich und je nach Rechner
    /// unterschiedlich lang. Über die Zeit lässt sich außerdem etwas
    /// dazwischenschieben — etwa die Anlage über den Socket in einen
    /// bestimmten Zustand fahren und *den* aufnehmen.
    pub warte_bis: std::time::Instant,
    pub angefordert: bool,
}

pub struct MusikApp {
    /// Fertige Ladeaufträge vom Arbeiter-Thread.
    pub ergebnisse: std::sync::mpsc::Receiver<crate::sammlung::Ergebnis>,
    /// Der gemeinsame Steuerraum. Die Oberfläche ist einer von mehreren
    /// Bedienern — ein Skript am Socket sieht dieselben Werte.
    pub pult: Arc<Mutex<Steuerpult>>,
    /// Hält den Audio-Stream am Leben. Fällt er weg, verstummt die Ausgabe —
    /// deshalb liegt er hier, auch wenn sonst niemand ihn anfasst.
    #[allow(dead_code)]
    pub output: Option<Output>,
    pub audio_hinweis: String,
    pub decks: Vec<DeckUi>,
    pub suche: String,
    pub treffer: Vec<Treffer>,
    pub status: String,
    pub screenshot: Option<Screenshot>,
}

impl eframe::App for MusikApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Abgelöste Quellen abholen, sonst nimmt der Audio-Thread irgendwann
        // keine neuen Tracks mehr an.
        if let Ok(mut pult) = self.pult.lock() {
            pult.handle_mut().collect_retired();
        }
        self.ladeergebnisse_einsetzen();

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
    /// Setzt fertig geladene Tracks ein.
    ///
    /// Im UI-Thread, weil die Wellenform-Spitzen hier liegen und weil das
    /// Einsetzen selbst kurz ist — die teure Arbeit ist längst getan.
    fn ladeergebnisse_einsetzen(&mut self) {
        while let Ok(ergebnis) = self.ergebnisse.try_recv() {
            let deck = ergebnis.deck;

            match ergebnis.ausgang {
                Ok(fertig) => {
                    let Ok(mut pult) = self.pult.lock() else {
                        return;
                    };
                    let Some(kanal) = pult.decks().get(deck).map(|d| d.kanal) else {
                        continue;
                    };

                    if !pult
                        .handle_mut()
                        .load(kanal, Box::new(audio_engine::DeckSource::new(fertig.voice)))
                    {
                        // Die Ladeschlange ist voll. Das ist ein echter
                        // Fehlschlag und darf nicht als Erfolg dastehen.
                        if let Some(e) = pult.deck_mut(deck) {
                            e.lade_status = "fehler: Ladeschlange voll".into();
                        }
                        self.status = "Ladeschlange voll — gleich noch einmal".into();
                        continue;
                    }

                    if let Some(e) = pult.deck_mut(deck) {
                        e.titel = fertig.titel.clone();
                        e.artist = fertig.artist;
                        e.frames = fertig.frames;
                        e.lade_status = "bereit".into();
                    }
                    drop(pult);

                    if let Some(ui) = self.decks.get_mut(deck) {
                        ui.peaks = fertig.peaks;
                        ui.frames = fertig.frames;
                    }
                    self.status = format!("{} → Deck {}", fertig.titel, deck + 1);
                }
                Err(text) => {
                    if let Ok(mut pult) = self.pult.lock() {
                        if let Some(e) = pult.deck_mut(deck) {
                            e.lade_status = format!("fehler: {text}");
                        }
                    }
                    self.status = format!("{} ließ sich nicht laden: {text}", ergebnis.pfad);
                }
            }
        }
    }

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
        let (titel, artist) = match self.pult.lock() {
            Ok(pult) => match pult.decks().get(index) {
                Some(d) => (d.titel.clone(), d.artist.clone()),
                None => (String::new(), String::new()),
            },
            Err(_) => (String::new(), String::new()),
        };

        let (dauer, position, bpm, keylock, laeuft) = {
            let deck = &self.decks[index];
            (
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

        // Einmal je Bild sperren. Der Audio-Thread ist daran nicht beteiligt —
        // er sieht nur die lock-freie Schlange, die das Pult füllt. Hier
        // konkurriert die Oberfläche nur mit anderen Bedienern, und die sind
        // langsam.
        let Ok(mut pult) = self.pult.lock() else {
            ui.label(RichText::new("Steuerpult nicht erreichbar").color(theme::WARNUNG));
            return;
        };

        ui.horizontal_top(|ui| {
            for i in 0..pult.kanaele().len() {
                kanalzug(ui, &mut pult, i, theme::deck_farbe(i));
            }
            ui.separator();
            summe(ui, &mut pult);
        });
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

            if self.treffer.is_empty() && self.suche.is_empty() {
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
                            ui.label(&eintrag.titel);

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

    /// Sucht über das Pult — denselben Weg, den ein Agent nimmt.
    fn suchen(&mut self) {
        if let Ok(pult) = self.pult.lock() {
            self.treffer = pult.suche(&self.suche);
        }
    }

    fn mischbar_suchen(&mut self, deck: usize) {
        let Some(bpm) = self.decks.get(deck).and_then(|d| d.state.effective_bpm()) else {
            self.status = "Das Deck hat kein Tempo".into();
            return;
        };

        if let Ok(pult) = self.pult.lock() {
            self.treffer = pult.suche_mischbar(bpm);
        }
        self.status = format!("Mischbar mit {bpm:.2} BPM: {} Treffer", self.treffer.len());
    }

    /// Lädt über denselben Weg, den ein Agent nimmt.
    ///
    /// Vorher hatte die Oberfläche ihren eigenen Ladepfad. Zwei Wege zum
    /// selben Ziel heißt zwei Stellen, an denen es schiefgehen kann — und die
    /// seltener benutzte fällt seltener auf.
    fn laden(&mut self, deck: usize, eintrag: &Treffer) {
        let ergebnis = {
            let Ok(mut pult) = self.pult.lock() else {
                return;
            };
            let schluessel = Schluessel::neu(Gruppe::Deck(deck), "load");
            pult.ausloesen(&schluessel, Some(&eintrag.pfad))
        };

        self.status = match ergebnis {
            Ok(_) => format!("{} wird geladen …", eintrag.titel),
            Err(e) => format!("Laden fehlgeschlagen: {e}"),
        };
    }

    /// Nimmt nach ein paar Bildern ein Abbild auf und beendet sich.
    fn screenshot_schritt(&mut self, ctx: &egui::Context) {
        let Some(auftrag) = self.screenshot.as_mut() else {
            return;
        };

        if !auftrag.angefordert {
            if std::time::Instant::now() < auftrag.warte_bis {
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

/// Ein Schieberegler, der an einem Control hängt.
///
/// Die Oberfläche kennt weder Bereich noch Einheit noch Bedeutung — das steht
/// alles im Katalog, und der Regler holt es sich von dort. Ein neues Control
/// ist damit ohne eine Zeile hier bedienbar, und der Hilfetext, den ein Agent
/// über `list` bekommt, ist derselbe, der hier als Tooltip erscheint. Zwei
/// Beschreibungen, die auseinanderlaufen könnten, gibt es nicht.
fn regler(ui: &mut Ui, pult: &mut Steuerpult, key: &Schluessel, name: &str, senkrecht: bool) {
    let Some(b) = pult.beschreibung(key) else {
        return;
    };
    let Some((min, max)) = b.bereich else {
        return;
    };
    let Ok(Wert::Zahl(aktuell)) = pult.lies(key) else {
        return;
    };

    let mut wert = aktuell;
    let mut schieber = egui::Slider::new(&mut wert, min..=max).show_value(false);
    schieber = if senkrecht {
        schieber.vertical()
    } else {
        schieber.text(RichText::new(name).size(10.0))
    };

    let antwort = ui.add(schieber).on_hover_text(b.text);
    if antwort.changed() {
        let _ = pult.schreibe(key, Wert::Zahl(wert));
    }
    // Doppelklick stellt zurück auf den Anfangswert des Bereichs bzw. die
    // Mitte — bei einem bipolaren Control ist das die Raste.
    if antwort.double_clicked() {
        let zurueck = if b.einheit == Einheit::Bipolar {
            0.0
        } else if min <= 1.0 && max >= 1.0 {
            1.0
        } else {
            min
        };
        let _ = pult.schreibe(key, Wert::Zahl(zurueck));
    }
}

fn kanalzug(ui: &mut Ui, pult: &mut Steuerpult, index: usize, farbe: Color32) {
    egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(7.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            // Senkrecht, sonst stünden die Regler eines Zuges nebeneinander
            // statt übereinander — der Rahmen erbt hier ein waagerechtes
            // Layout vom Mixer.
            ui.vertical(|ui| kanalzug_inhalt(ui, pult, index, farbe));
        });
}

fn kanalzug_inhalt(ui: &mut Ui, pult: &mut Steuerpult, index: usize, farbe: Color32) {
    let Some(kanal) = pult.kanaele().get(index) else {
        return;
    };
    let name = kanal.name.clone();
    let cue_an = kanal.cue;

    ui.set_width(KANALBREITE);
    ui.label(RichText::new(&name).color(farbe).strong().size(12.0));
    ui.spacing_mut().slider_width = KANALBREITE - 34.0;

    let gruppe = Gruppe::Kanal(index);
    for (element, beschriftung) in [
        ("trim", "TRIM"),
        ("eq_high", "HI"),
        ("eq_mid", "MID"),
        ("eq_low", "LOW"),
        ("filter", "FLT"),
    ] {
        regler(
            ui,
            pult,
            &Schluessel::neu(gruppe, element),
            beschriftung,
            false,
        );
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let knopf = ui.add(
                egui::Button::new(RichText::new("CUE").size(10.0))
                    .fill(if cue_an {
                        theme::CUE
                    } else {
                        theme::PANEL_HELL
                    })
                    .min_size(egui::vec2(42.0, 22.0)),
            );
            if knopf.clicked() {
                let _ = pult.schreibe(&Schluessel::neu(gruppe, "cue"), Wert::Schalter(!cue_an));
            }
        });

        // Der Linefader steht senkrecht wie am Gerät.
        ui.spacing_mut().slider_width = FADERHOEHE;
        regler(ui, pult, &Schluessel::neu(gruppe, "fader"), "", true);
    });
}

fn summe(ui: &mut Ui, pult: &mut Steuerpult) {
    egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(7.0)
        .corner_radius(4.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                ui.spacing_mut().slider_width = 214.0;
                ui.label(RichText::new("SUMME").strong().size(12.0));

                let g = Gruppe::Master;
                regler(
                    ui,
                    pult,
                    &Schluessel::neu(g, "crossfader"),
                    "A « Crossfader » B",
                    false,
                );
                regler(
                    ui,
                    pult,
                    &Schluessel::neu(g, "crossfader_curve"),
                    "Kurve weich » hart",
                    false,
                );
                ui.separator();
                regler(ui, pult, &Schluessel::neu(g, "gain"), "MASTER", false);
                regler(
                    ui,
                    pult,
                    &Schluessel::neu(g, "cue_gain"),
                    "KOPFHÖRER",
                    false,
                );
                regler(
                    ui,
                    pult,
                    &Schluessel::neu(g, "cue_mix"),
                    "CUE « Mix » MASTER",
                    false,
                );
            });
        });
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
}
