//! Suchen und Laden für das Steuerpult.
//!
//! Das Pult darf beim Laden nicht blockieren: Dekodieren und Analysieren
//! dauern Sekunden, und es liegt währenddessen unter einem Mutex, an dem die
//! Oberfläche hängt. Ein `do deck1.load` würde das Bild einfrieren.
//!
//! Deshalb drei Stationen. Der Auftrag wird angenommen und weitergereicht; ein
//! Arbeiter-Thread macht die langsame Arbeit ohne jedes Schloss; der
//! UI-Thread setzt das Ergebnis ein, weil dort ohnehin die Wellenform-Spitzen
//! liegen. Wer wissen will, wann der Track liegt, fragt `deckN.load_status`
//! oder abonniert es.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use analysis::peaks::PeakLevel;
use analysis::Store;
use audio_core::deck::{DeckState, Voice};
use audio_core::{Beatgrid, Tonart, Track};
use control::{Sammlung, Treffer};
use library::{CueKind, CueRecord, Library, Query, TrackRecord};

/// Ein angenommener Auftrag auf dem Weg zum Arbeiter.
pub struct Auftrag {
    pub deck: usize,
    pub pfad: String,
    pub state: Arc<DeckState>,
    pub sample_rate: u32,
    /// Hot Cues aus der Sammlung, als (Nummer ab 0, Frames).
    ///
    /// Gelesen wird beim Annehmen des Auftrags, nicht im Arbeiter: Dort liegt
    /// die Sammlung, und acht Zeilen SQLite kosten nichts.
    pub cues: Vec<(usize, u64)>,
    /// Beatgrid aus der Sammlung. Schlägt das der Analyse — was hier steht,
    /// kann aus Traktor stammen oder von Hand korrigiert sein.
    pub grid: Option<Beatgrid>,
}

/// Was der Arbeiter zurückgibt.
pub struct Ergebnis {
    pub deck: usize,
    pub pfad: String,
    pub ausgang: Result<Fertig, String>,
}

pub struct Fertig {
    pub voice: Voice,
    pub peaks: Vec<PeakLevel>,
    pub frames: u64,
    pub titel: String,
    pub artist: String,
    pub tonart: Option<Tonart>,
}

/// Die Umsetzung, die das Pult bekommt.
pub struct AppSammlung {
    library: Option<Library>,
    auftraege: Sender<Auftrag>,
    /// Deck-Zustände, damit ein Auftrag weiß, worauf er wirkt.
    decks: Vec<(Arc<DeckState>, u32)>,
}

impl AppSammlung {
    /// Was die Sammlung über einen Track weiß, das ein Deck braucht.
    ///
    /// Leer, wenn keine Sammlung offen ist oder der Track nicht darin steht —
    /// dann trägt allein die Analyse, und das ist kein Fehler.
    fn gespeichertes(&self, pfad: &str, rate: u32) -> (Vec<(usize, u64)>, Option<Beatgrid>) {
        let Some(lib) = self.library.as_ref() else {
            return (Vec::new(), None);
        };
        let Ok(Some(eintrag)) = lib.track_by_path(pfad) else {
            return (Vec::new(), None);
        };

        let grid = eintrag.beatgrid(rate);
        let cues = match eintrag.id.map(|id| lib.cues(id)) {
            Some(Ok(zeilen)) => zeilen
                .iter()
                .filter_map(|c| {
                    let nummer = c.hotcue? as usize;
                    (nummer < audio_core::deck::HOT_CUES).then(|| (nummer, c.frame(rate)))
                })
                .collect(),
            _ => Vec::new(),
        };

        (cues, grid)
    }

    pub fn neu(
        library: Option<Library>,
        decks: Vec<(Arc<DeckState>, u32)>,
        cache: PathBuf,
    ) -> (AppSammlung, Receiver<Ergebnis>) {
        let (auftraege, eingang) = std::sync::mpsc::channel::<Auftrag>();
        let (ausgang, ergebnisse) = std::sync::mpsc::channel::<Ergebnis>();

        std::thread::Builder::new()
            .name("lader".into())
            .spawn(move || arbeiten(eingang, ausgang, cache))
            .expect("Lade-Thread ließ sich nicht starten");

        (
            AppSammlung {
                library,
                auftraege,
                decks,
            },
            ergebnisse,
        )
    }
}

impl Sammlung for AppSammlung {
    fn suchen(&self, text: &str, grenze: usize) -> Vec<Treffer> {
        let Some(lib) = self.library.as_ref() else {
            return Vec::new();
        };

        let mut query = if text.is_empty() {
            Query::default()
        } else {
            Query::text(text)
        };
        query.limit = Some(grenze as u32);

        lib.search(&query)
            .unwrap_or_default()
            .into_iter()
            .map(treffer_aus)
            .collect()
    }

    fn suchen_mischbar(&self, bpm: f32, grenze: usize) -> Vec<Treffer> {
        let Some(lib) = self.library.as_ref() else {
            return Vec::new();
        };

        let mut query = Query::mixable_with(bpm, 0.06);
        query.limit = Some(grenze as u32);
        lib.search(&query)
            .unwrap_or_default()
            .into_iter()
            .map(treffer_aus)
            .collect()
    }

    fn suchen_harmonisch(&self, tonart: Tonart, grenze: usize) -> Vec<Treffer> {
        let Some(lib) = self.library.as_ref() else {
            return Vec::new();
        };

        let mut query = Query::harmonic_with(tonart);
        query.limit = Some(grenze as u32);
        lib.search(&query)
            .unwrap_or_default()
            .into_iter()
            .map(treffer_aus)
            .collect()
    }

    fn playlists(&self) -> Vec<String> {
        let Some(lib) = self.library.as_ref() else {
            return Vec::new();
        };
        lib.playlists()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, name)| name)
            .collect()
    }

    fn playlist(&self, name: &str, grenze: usize) -> Vec<Treffer> {
        let Some(lib) = self.library.as_ref() else {
            return Vec::new();
        };
        let Ok(listen) = lib.playlists() else {
            return Vec::new();
        };
        let Some((id, _)) = listen.into_iter().find(|(_, n)| n == name) else {
            return Vec::new();
        };

        lib.playlist_tracks(id)
            .unwrap_or_default()
            .into_iter()
            .take(grenze)
            .map(treffer_aus)
            .collect()
    }

    fn laden(&self, deck: usize, pfad: &str) -> Result<(), String> {
        let Some((state, sample_rate)) = self.decks.get(deck) else {
            return Err(format!("deck{} gibt es nicht", deck + 1));
        };

        // Früh prüfen: Ein Tippfehler im Pfad soll sofort auffallen und nicht
        // erst Sekunden später als Status am Deck.
        if !std::path::Path::new(pfad).is_file() {
            return Err(format!("{pfad} gibt es nicht"));
        }

        let (cues, grid) = self.gespeichertes(pfad, *sample_rate);

        self.auftraege
            .send(Auftrag {
                deck,
                pfad: pfad.to_string(),
                state: Arc::clone(state),
                sample_rate: *sample_rate,
                cues,
                grid,
            })
            .map_err(|_| "der Lader läuft nicht mehr".to_string())
    }

    fn hot_cues_speichern(&self, pfad: &str, cues: &[(usize, f64)]) -> Result<(), String> {
        let Some(lib) = self.library.as_ref() else {
            return Err("keine Sammlung geöffnet — mit --db starten".into());
        };
        let id = lib
            .track_id_by_path(pfad)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{pfad} steht nicht in der Sammlung"))?;

        let zeilen: Vec<CueRecord> = cues
            .iter()
            .map(|(nummer, sekunden)| CueRecord {
                id: None,
                track_id: id,
                hotcue: Some(*nummer as u8),
                position_ms: sekunden * 1_000.0,
                name: None,
                kind: CueKind::Cue,
            })
            .collect();

        // Nur die Hot Cues — der Grid-Marker aus dem Traktor-Import liegt in
        // derselben Tabelle und geht das Deck nichts an.
        lib.replace_hot_cues(id, &zeilen).map_err(|e| e.to_string())
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

fn treffer_aus(eintrag: TrackRecord) -> Treffer {
    Treffer {
        titel: anzeigename(&eintrag),
        artist: eintrag.artist,
        bpm: eintrag.bpm,
        // Was sich nicht lesen lässt, wird nicht gezeigt — eine falsch
        // gedeutete Tonart wäre schlimmer als eine fehlende.
        tonart: eintrag.musical_key.as_deref().and_then(Tonart::parse),
        pfad: eintrag.path,
    }
}

/// Der Arbeiter. Nimmt sich Zeit, hält kein Schloss.
fn arbeiten(eingang: Receiver<Auftrag>, ausgang: Sender<Ergebnis>, cache: PathBuf) {
    let store = Store::new(&cache);

    for auftrag in eingang {
        let ergebnis = fertigen(&auftrag, &store);
        if ausgang
            .send(Ergebnis {
                deck: auftrag.deck,
                pfad: auftrag.pfad,
                ausgang: ergebnis,
            })
            .is_err()
        {
            // Niemand hört mehr zu — die Anwendung wird beendet.
            return;
        }
    }
}

fn fertigen(auftrag: &Auftrag, store: &Store) -> Result<Fertig, String> {
    let track = Track::decode_file(std::path::Path::new(&auftrag.pfad))
        .map_err(|e| format!("nicht lesbar: {e}"))?
        .resampled_to(auftrag.sample_rate);

    let (analyse, gerechnet) = analysis::analyze_cached(&track, store);
    if gerechnet {
        let _ = store.save(&analyse);
    }

    let titel = std::path::Path::new(&auftrag.pfad)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| auftrag.pfad.clone());

    let peaks = analyse
        .peaks
        .iter()
        .filter_map(|p| p.to_level())
        .collect::<Vec<_>>();
    let frames = track.frames() as u64;

    // Der neue Track startet gestoppt am Anfang — ein Deck, das beim Laden
    // losläuft, ist ein Unfall. Auch und gerade, wenn ein Agent geladen hat.
    auftrag.state.set_playing(false);
    auftrag.state.seek_frames(0);
    auftrag.state.set_loop_active(false);

    // Erst alles leeren, dann das Gespeicherte einsetzen. Ohne das Leeren
    // stünden die Cues des vorigen Tracks noch da, wo der neue keine hat.
    for i in 0..audio_core::deck::HOT_CUES {
        auftrag.state.set_cue(i, None);
    }
    for (nummer, frame) in &auftrag.cues {
        auftrag.state.set_cue(*nummer, Some(*frame));
    }

    // Was in der Sammlung steht, schlägt die frische Analyse: Es kann aus
    // Traktor stammen oder von Hand korrigiert sein, und beides weiß mehr als
    // ein Detektor.
    let grid = auftrag.grid.or_else(|| {
        analyse
            .bpm
            .map(|bpm| Beatgrid::new(bpm, analyse.beat_anchor_frames.unwrap_or(0), 1.0))
    });
    auftrag.state.set_grid(grid);

    Ok(Fertig {
        voice: Voice::new(Arc::new(track), Arc::clone(&auftrag.state)),
        peaks,
        frames,
        titel,
        artist: String::new(),
        tonart: analyse.tonart(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ohne_titel_bleibt_der_dateiname_stehen() {
        let mut eintrag =
            TrackRecord::from_path("/musik/Ordner/Alpenglühen - Vier auf die Eins.wav");
        assert_eq!(anzeigename(&eintrag), "Alpenglühen - Vier auf die Eins");

        eintrag.title = Some("Richtiger Titel".into());
        assert_eq!(anzeigename(&eintrag), "Richtiger Titel");

        // Ein leerer Tag ist so gut wie keiner.
        eintrag.title = Some("   ".into());
        assert_eq!(anzeigename(&eintrag), "Alpenglühen - Vier auf die Eins");
    }
}
