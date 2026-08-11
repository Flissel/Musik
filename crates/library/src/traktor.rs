//! Import einer Traktor-Sammlung (`collection.nml`).
//!
//! Wer von Traktor kommt, bringt Jahre an Cue-Points und Beatgrids mit. Die neu
//! zu setzen wäre die unangenehmste Art, ein neues Werkzeug zu beginnen —
//! deshalb steht der Import früh und nicht am Ende.
//!
//! Die `.nml` ist XML und enthält **keine Audiodaten**, nur Pfade und
//! Metadaten. Positionen stehen darin in Millisekunden, was zur Ablage in
//! dieser Library passt.
//!
//! ## Was hier ungeprüft ist
//!
//! Das Format ist nicht offiziell dokumentiert. Feldbedeutungen stammen aus
//! öffentlichen Beschreibungen und sind hier **nur gegen selbst geschriebene
//! Beispiele getestet**, nicht gegen eine echte Traktor-Sammlung. Betroffen
//! sind vor allem:
//!
//! - die Zuordnung der `CUE_V2`-Typen (siehe [`cue_kind`]),
//! - die Nummerierung in `MUSICAL_KEY` (siehe [`traktor_key_name`]),
//! - der Umgang mit `VOLUME` unter Windows (siehe [`build_path`]).
//!
//! Der erste Lauf gegen eine echte Sammlung gehört deshalb stichprobenartig
//! nachgesehen, bevor man sich darauf verlässt.

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::db::Library;
use crate::error::Result;
use crate::model::{CueKind, CueRecord, Source, TrackRecord};

/// Was der Import getan hat.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub entries_seen: usize,
    pub tracks_imported: usize,
    pub cues_imported: usize,
    /// Einträge ohne verwertbaren Pfad — die gibt es in echten Sammlungen.
    pub skipped_without_location: usize,
}

/// Ein gelesener Eintrag, noch ohne Datenbank-ID.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedEntry {
    pub track: TrackRecord,
    pub cues: Vec<CueRecord>,
}

/// Liest eine `collection.nml` und schreibt sie in die Library.
pub fn import_nml(xml: &str, library: &Library) -> Result<ImportReport> {
    let entries = parse_nml(xml)?;
    let mut report = ImportReport {
        entries_seen: entries.len(),
        ..Default::default()
    };

    for entry in entries {
        if entry.track.path.is_empty() {
            report.skipped_without_location += 1;
            continue;
        }

        let id = library.upsert_track(&entry.track)?;
        report.tracks_imported += 1;

        if !entry.cues.is_empty() {
            let cues: Vec<CueRecord> = entry
                .cues
                .into_iter()
                .map(|mut c| {
                    c.track_id = id;
                    c
                })
                .collect();
            report.cues_imported += cues.len();
            library.replace_cues(id, &cues)?;
        }
    }

    Ok(report)
}

/// Reines Parsen ohne Datenbank — der testbare Teil.
pub fn parse_nml(xml: &str) -> Result<Vec<ImportedEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut current: Option<ImportedEntry> = None;
    let mut dir = String::new();
    let mut file = String::new();
    let mut volume = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"ENTRY" => {
                        let mut track = TrackRecord::from_path("");
                        track.title = attr(&e, b"TITLE");
                        track.artist = attr(&e, b"ARTIST");
                        current = Some(ImportedEntry {
                            track,
                            cues: Vec::new(),
                        });
                        dir.clear();
                        file.clear();
                        volume.clear();
                    }
                    b"LOCATION" => {
                        dir = attr(&e, b"DIR").unwrap_or_default();
                        file = attr(&e, b"FILE").unwrap_or_default();
                        volume = attr(&e, b"VOLUME").unwrap_or_default();
                    }
                    b"ALBUM" => {
                        if let Some(entry) = current.as_mut() {
                            entry.track.album = attr(&e, b"TITLE");
                        }
                    }
                    b"INFO" => {
                        if let Some(entry) = current.as_mut() {
                            entry.track.genre = attr(&e, b"GENRE");
                            entry.track.musical_key = attr(&e, b"KEY");
                            entry.track.duration_secs = attr(&e, b"PLAYTIME_FLOAT")
                                .and_then(|v| v.parse().ok())
                                .or_else(|| {
                                    attr(&e, b"PLAYTIME").and_then(|v| v.parse::<f64>().ok())
                                });
                        }
                    }
                    b"TEMPO" => {
                        if let Some(entry) = current.as_mut() {
                            entry.track.bpm = attr(&e, b"BPM").and_then(|v| v.parse().ok());
                        }
                    }
                    b"MUSICAL_KEY" => {
                        if let Some(entry) = current.as_mut() {
                            if entry.track.musical_key.is_none() {
                                entry.track.musical_key = attr(&e, b"VALUE")
                                    .and_then(|v| v.parse::<u8>().ok())
                                    .and_then(traktor_key_name)
                                    .map(str::to_string);
                            }
                        }
                    }
                    b"CUE_V2" => {
                        if let Some(entry) = current.as_mut() {
                            if let Some(cue) = read_cue(&e) {
                                // Der Grid-Marker ist kein Cue, sondern der
                                // Beat-Anker — er gehört an den Track.
                                if cue.kind == CueKind::Grid {
                                    entry.track.beat_anchor_ms = Some(cue.position_ms);
                                }
                                entry.cues.push(cue);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"ENTRY" => {
                if let Some(mut entry) = current.take() {
                    entry.track.path = build_path(&volume, &dir, &file);
                    entry.track.source = Source::File;
                    entries.push(entry);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

fn attr(element: &BytesStart, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Explicit1_0).ok())
        .map(|v| v.into_owned())
        .filter(|v| !v.is_empty())
}

fn read_cue(element: &BytesStart) -> Option<CueRecord> {
    let start: f64 = attr(element, b"START")?.parse().ok()?;
    let typ: i32 = attr(element, b"TYPE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let hotcue: i32 = attr(element, b"HOTCUE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);

    Some(CueRecord {
        id: None,
        track_id: 0,
        hotcue: (0..=127).contains(&hotcue).then_some(hotcue as u8),
        position_ms: start,
        name: attr(element, b"NAME"),
        kind: cue_kind(typ),
    })
}

/// Typnummern der `CUE_V2`-Marker.
///
/// Aus öffentlichen Beschreibungen des Formats; gegen eine echte Sammlung
/// ungeprüft. Unbekannte Werte landen als gewöhnlicher Cue, was schlimmstenfalls
/// einen Marker zu viel auf einer Taste bedeutet — nie einen Datenverlust.
fn cue_kind(typ: i32) -> CueKind {
    match typ {
        1 => CueKind::FadeIn,
        2 => CueKind::FadeOut,
        3 => CueKind::Load,
        4 => CueKind::Grid,
        5 => CueKind::Loop,
        _ => CueKind::Cue,
    }
}

/// Tonartnummern aus `MUSICAL_KEY`.
///
/// 0–11 Dur aufsteigend ab C, 12–23 Moll ebenso. Aus öffentlichen
/// Beschreibungen; gegen eine echte Sammlung ungeprüft. Steht in `INFO@KEY` ein
/// Text, wird der bevorzugt — der stammt direkt aus Traktor.
fn traktor_key_name(value: u8) -> Option<&'static str> {
    const DUR: [&str; 12] = [
        "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
    ];
    const MOLL: [&str; 12] = [
        "Cm", "Dbm", "Dm", "Ebm", "Em", "Fm", "Gbm", "Gm", "Abm", "Am", "Bbm", "Bm",
    ];

    match value {
        0..=11 => Some(DUR[value as usize]),
        12..=23 => Some(MOLL[(value - 12) as usize]),
        _ => None,
    }
}

/// Baut den Dateipfad aus `VOLUME`, `DIR` und `FILE`.
///
/// Traktor trennt Verzeichnisse mit `/:` statt mit `/`. `VOLUME` ist unter
/// Windows der Laufwerksbuchstabe (`C:`), unter macOS ein Datenträgername —
/// deshalb wird nur übernommen, was auf einen Doppelpunkt endet. Ein
/// macOS-Datenträgername ergäbe sonst einen Pfad, den es nicht gibt.
fn build_path(volume: &str, dir: &str, file: &str) -> String {
    if file.is_empty() {
        return String::new();
    }

    let mut pfad = dir.replace("/:", "/");
    if !pfad.ends_with('/') && !pfad.is_empty() {
        pfad.push('/');
    }
    pfad.push_str(file);

    if volume.ends_with(':') {
        format!("{volume}{pfad}")
    } else {
        pfad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEISPIEL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<NML VERSION="19">
  <COLLECTION ENTRIES="2">
    <ENTRY MODIFIED_DATE="2026/1/1" TITLE="Fear Paralysis" ARTIST="Rebekah">
      <LOCATION DIR="/:Users/:felix/:Music/:" FILE="fear.mp3" VOLUME="Macintosh HD"/>
      <ALBUM TITLE="Fear Paralysis EP"/>
      <INFO GENRE="Techno" PLAYTIME="384" KEY="Am"/>
      <TEMPO BPM="135.000000" BPM_QUALITY="100.000000"/>
      <CUE_V2 NAME="AutoGrid" DISPLAY_ORDER="0" TYPE="4" START="122.5" LEN="0" HOTCUE="-1"/>
      <CUE_V2 NAME="Drop" DISPLAY_ORDER="1" TYPE="0" START="64000.25" LEN="0" HOTCUE="1"/>
    </ENTRY>
    <ENTRY TITLE="Ohne Pfad" ARTIST="Niemand">
      <TEMPO BPM="120.000000"/>
    </ENTRY>
  </COLLECTION>
</NML>"#;

    #[test]
    fn liest_metadaten_und_tempo() {
        let entries = parse_nml(BEISPIEL).unwrap();
        assert_eq!(entries.len(), 2);

        let t = &entries[0].track;
        assert_eq!(t.title.as_deref(), Some("Fear Paralysis"));
        assert_eq!(t.artist.as_deref(), Some("Rebekah"));
        assert_eq!(t.album.as_deref(), Some("Fear Paralysis EP"));
        assert_eq!(t.genre.as_deref(), Some("Techno"));
        assert_eq!(t.musical_key.as_deref(), Some("Am"));
        assert_eq!(t.bpm, Some(135.0));
        assert_eq!(t.duration_secs, Some(384.0));
    }

    #[test]
    fn baut_den_pfad_aus_traktors_trennzeichen() {
        let entries = parse_nml(BEISPIEL).unwrap();
        assert_eq!(entries[0].track.path, "/Users/felix/Music/fear.mp3");
    }

    #[test]
    fn windows_laufwerk_kommt_davor_ein_mac_datentraeger_nicht() {
        assert_eq!(
            build_path("C:", "/:Musik/:Techno/:", "a.mp3"),
            "C:/Musik/Techno/a.mp3"
        );
        assert_eq!(
            build_path("Macintosh HD", "/:Musik/:", "a.mp3"),
            "/Musik/a.mp3"
        );
        assert_eq!(build_path("C:", "/:Musik/:", ""), "");
    }

    #[test]
    fn der_grid_marker_wird_zum_beat_anker() {
        let entries = parse_nml(BEISPIEL).unwrap();
        assert_eq!(entries[0].track.beat_anchor_ms, Some(122.5));

        let grid = entries[0]
            .track
            .beatgrid(44_100)
            .expect("kein Beatgrid aufgebaut");
        assert!((grid.bpm - 135.0).abs() < 1e-4);
        assert_eq!(grid.anchor_frames, 5_402);
    }

    #[test]
    fn hot_cues_behalten_nummer_und_position() {
        let entries = parse_nml(BEISPIEL).unwrap();
        let cues = &entries[0].cues;
        assert_eq!(cues.len(), 2);

        let grid = cues.iter().find(|c| c.kind == CueKind::Grid).unwrap();
        assert_eq!(grid.hotcue, None, "der Grid-Marker liegt auf keiner Taste");

        let drop = cues.iter().find(|c| c.kind == CueKind::Cue).unwrap();
        assert_eq!(drop.hotcue, Some(1));
        assert_eq!(drop.position_ms, 64_000.25);
        assert_eq!(drop.name.as_deref(), Some("Drop"));
        assert_eq!(drop.frame(44_100), 2_822_411);
    }

    #[test]
    fn eintraege_ohne_pfad_werden_gezaehlt_nicht_verschluckt() {
        let lib = Library::open_in_memory().unwrap();
        let report = import_nml(BEISPIEL, &lib).unwrap();

        assert_eq!(report.entries_seen, 2);
        assert_eq!(report.tracks_imported, 1);
        assert_eq!(report.skipped_without_location, 1);
        assert_eq!(report.cues_imported, 2);
        assert_eq!(lib.track_count().unwrap(), 1);
    }

    #[test]
    fn import_landet_vollstaendig_in_der_datenbank() {
        let lib = Library::open_in_memory().unwrap();
        import_nml(BEISPIEL, &lib).unwrap();

        let t = lib
            .track_by_path("/Users/felix/Music/fear.mp3")
            .unwrap()
            .expect("Track nicht gespeichert");
        assert_eq!(t.bpm, Some(135.0));
        assert_eq!(t.beat_anchor_ms, Some(122.5));

        let cues = lib.cues(t.id.unwrap()).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].position_ms, 122.5, "nicht nach Position sortiert");
    }

    #[test]
    fn zweimal_importieren_verdoppelt_nichts() {
        let lib = Library::open_in_memory().unwrap();
        import_nml(BEISPIEL, &lib).unwrap();
        import_nml(BEISPIEL, &lib).unwrap();

        assert_eq!(lib.track_count().unwrap(), 1);

        let t = lib
            .track_by_path("/Users/felix/Music/fear.mp3")
            .unwrap()
            .unwrap();
        assert_eq!(lib.cues(t.id.unwrap()).unwrap().len(), 2, "Cues verdoppelt");
    }

    #[test]
    fn numerische_tonart_wird_nur_ohne_textangabe_genutzt() {
        let xml = r#"<NML><COLLECTION>
          <ENTRY TITLE="X"><LOCATION DIR="/:a/:" FILE="x.mp3"/>
            <MUSICAL_KEY VALUE="21"/>
          </ENTRY></COLLECTION></NML>"#;

        let entries = parse_nml(xml).unwrap();
        assert_eq!(entries[0].track.musical_key.as_deref(), Some("Am"));

        assert_eq!(traktor_key_name(0), Some("C"));
        assert_eq!(traktor_key_name(12), Some("Cm"));
        assert_eq!(traktor_key_name(99), None);
    }

    #[test]
    fn kaputtes_xml_wird_gemeldet_statt_zu_paniken() {
        assert!(parse_nml("<NML><COLLECTION><ENTRY").is_err());
    }

    #[test]
    fn leere_sammlung_ist_kein_fehler() {
        let entries = parse_nml(r#"<NML VERSION="19"><COLLECTION ENTRIES="0"/></NML>"#).unwrap();
        assert!(entries.is_empty());
    }
}
