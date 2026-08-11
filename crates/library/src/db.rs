//! SQLite-Ablage der Sammlung.
//!
//! Eine Datei, kein Server. Für eine DJ-Library ist das genau richtig: Sie liegt
//! neben der Musik, lässt sich kopieren und sichern wie jede andere Datei, und
//! sie ist beim Auflegen sofort da statt nach dem Start eines Dienstes.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{LibraryError, Result};
use crate::model::{CueKind, CueRecord, Query, Source, TrackRecord};

/// Version des Schemas. Bei Änderungen hochzählen und eine Migration ergänzen.
const SCHEMA_VERSION: i32 = 1;

pub struct Library {
    conn: Connection,
}

impl Library {
    pub fn open(path: impl AsRef<Path>) -> Result<Library> {
        let library = Library {
            conn: Connection::open(path)?,
        };
        library.prepare()?;
        Ok(library)
    }

    pub fn open_in_memory() -> Result<Library> {
        let library = Library {
            conn: Connection::open_in_memory()?,
        };
        library.prepare()?;
        Ok(library)
    }

    fn prepare(&self) -> Result<()> {
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        self.conn.pragma_update(None, "journal_mode", "WAL")?;

        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            self.migrate_to_v1()?;
        }

        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(())
    }

    fn migrate_to_v1(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tracks (
                id                INTEGER PRIMARY KEY,
                path              TEXT NOT NULL UNIQUE,
                fingerprint       TEXT,
                title             TEXT,
                artist            TEXT,
                album             TEXT,
                genre             TEXT,
                duration_secs     REAL,
                bpm               REAL,
                beat_anchor_ms    REAL,
                musical_key       TEXT,
                license           TEXT,
                attribution       TEXT,
                source            TEXT NOT NULL DEFAULT 'file',
                added_at          INTEGER NOT NULL
            );

            -- Kein UNIQUE: Derselbe Klang darf zweimal im Dateisystem liegen,
            -- und das ist kein Fehler, den ein Constraint erzwingen sollte.
            CREATE INDEX IF NOT EXISTS idx_tracks_fingerprint ON tracks(fingerprint);
            CREATE INDEX IF NOT EXISTS idx_tracks_bpm ON tracks(bpm);

            CREATE TABLE IF NOT EXISTS cues (
                id            INTEGER PRIMARY KEY,
                track_id      INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                hotcue        INTEGER,
                position_ms   REAL NOT NULL,
                name          TEXT,
                kind          TEXT NOT NULL DEFAULT 'cue'
            );
            CREATE INDEX IF NOT EXISTS idx_cues_track ON cues(track_id);

            CREATE TABLE IF NOT EXISTS playlists (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL UNIQUE,
                created_at  INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS playlist_items (
                playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
                position    INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, position)
            );
            "#,
        )?;
        Ok(())
    }

    /// Legt einen Track an oder aktualisiert ihn. Schlüssel ist der Pfad.
    ///
    /// Felder, die im übergebenen Datensatz `None` sind, überschreiben
    /// vorhandene Werte **nicht** — sonst löschte ein erneuter Ordner-Scan die
    /// mühsam erarbeitete Analyse wieder weg.
    pub fn upsert_track(&self, track: &TrackRecord) -> Result<i64> {
        let now = unix_now();

        self.conn.execute(
            r#"
            INSERT INTO tracks
                (path, fingerprint, title, artist, album, genre, duration_secs,
                 bpm, beat_anchor_ms, musical_key, license, attribution, source, added_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(path) DO UPDATE SET
                fingerprint    = COALESCE(excluded.fingerprint, tracks.fingerprint),
                title          = COALESCE(excluded.title, tracks.title),
                artist         = COALESCE(excluded.artist, tracks.artist),
                album          = COALESCE(excluded.album, tracks.album),
                genre          = COALESCE(excluded.genre, tracks.genre),
                duration_secs  = COALESCE(excluded.duration_secs, tracks.duration_secs),
                bpm            = COALESCE(excluded.bpm, tracks.bpm),
                beat_anchor_ms = COALESCE(excluded.beat_anchor_ms, tracks.beat_anchor_ms),
                musical_key    = COALESCE(excluded.musical_key, tracks.musical_key),
                license        = COALESCE(excluded.license, tracks.license),
                attribution    = COALESCE(excluded.attribution, tracks.attribution),
                source         = excluded.source
            "#,
            params![
                track.path,
                track.fingerprint,
                track.title,
                track.artist,
                track.album,
                track.genre,
                track.duration_secs,
                track.bpm,
                track.beat_anchor_ms,
                track.musical_key,
                track.license,
                track.attribution,
                track.source.as_str(),
                now,
            ],
        )?;

        self.track_id_by_path(&track.path)?
            .ok_or_else(|| LibraryError::NotFound(track.path.clone()))
    }

    pub fn track_id_by_path(&self, path: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1",
                params![path],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn track(&self, id: i64) -> Result<Option<TrackRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("{SELECT_TRACK} WHERE id = ?1"),
                params![id],
                track_from_row,
            )
            .optional()?)
    }

    pub fn track_by_path(&self, path: &str) -> Result<Option<TrackRecord>> {
        Ok(self
            .conn
            .query_row(
                &format!("{SELECT_TRACK} WHERE path = ?1"),
                params![path],
                track_from_row,
            )
            .optional()?)
    }

    /// Alle Tracks mit diesem Inhalts-Hash — mehr als einer heißt: Dublette.
    pub fn tracks_by_fingerprint(&self, fingerprint: &str) -> Result<Vec<TrackRecord>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SELECT_TRACK} WHERE fingerprint = ?1"))?;
        let rows = stmt.query_map(params![fingerprint], track_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn track_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    pub fn search(&self, query: &Query) -> Result<Vec<TrackRecord>> {
        let mut sql = String::from(SELECT_TRACK);
        let mut wheres: Vec<String> = Vec::new();
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(text) = query.text.as_ref().filter(|t| !t.trim().is_empty()) {
            // Drei Spalten, derselbe Wert — SQLite nummeriert Platzhalter, also
            // dreimal binden statt einmal wiederverwenden.
            let n = args.len();
            wheres.push(format!(
                "(title LIKE ?{} OR artist LIKE ?{} OR album LIKE ?{})",
                n + 1,
                n + 2,
                n + 3
            ));

            let muster = format!("%{text}%");
            args.push(Box::new(muster.clone()));
            args.push(Box::new(muster.clone()));
            args.push(Box::new(muster));
        }

        if let Some(min) = query.bpm_min {
            wheres.push(format!("bpm >= ?{}", args.len() + 1));
            args.push(Box::new(min));
        }
        if let Some(max) = query.bpm_max {
            wheres.push(format!("bpm <= ?{}", args.len() + 1));
            args.push(Box::new(max));
        }
        if let Some(genre) = query.genre.as_ref() {
            wheres.push(format!("genre = ?{}", args.len() + 1));
            args.push(Box::new(genre.clone()));
        }

        if !wheres.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&wheres.join(" AND "));
        }
        sql.push_str(" ORDER BY artist, title");
        if let Some(limit) = query.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a.as_ref()).collect();
        let rows = stmt.query_map(refs.as_slice(), track_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Tracks, deren Rechteangaben fehlen, obwohl sie welche bräuchten.
    pub fn tracks_missing_attribution(&self) -> Result<Vec<TrackRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SELECT_TRACK} WHERE source = 'sample' AND \
             (license IS NULL OR TRIM(license) = '' OR \
              attribution IS NULL OR TRIM(attribution) = '')"
        ))?;
        let rows = stmt.query_map([], track_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Ersetzt alle Marker eines Tracks.
    pub fn replace_cues(&self, track_id: i64, cues: &[CueRecord]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM cues WHERE track_id = ?1", params![track_id])?;

        for cue in cues {
            tx.execute(
                "INSERT INTO cues (track_id, hotcue, position_ms, name, kind) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    track_id,
                    cue.hotcue.map(|h| h as i64),
                    cue.position_ms,
                    cue.name,
                    cue.kind.as_str(),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn cues(&self, track_id: i64) -> Result<Vec<CueRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, track_id, hotcue, position_ms, name, kind FROM cues \
             WHERE track_id = ?1 ORDER BY position_ms",
        )?;
        let rows = stmt.query_map(params![track_id], |row| {
            Ok(CueRecord {
                id: row.get(0)?,
                track_id: row.get(1)?,
                hotcue: row.get::<_, Option<i64>>(2)?.map(|v| v as u8),
                position_ms: row.get(3)?,
                name: row.get(4)?,
                kind: CueKind::parse(&row.get::<_, String>(5)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn create_playlist(&self, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO playlists (name, created_at) VALUES (?1, ?2)",
            params![name, unix_now()],
        )?;
        self.conn
            .query_row(
                "SELECT id FROM playlists WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    pub fn playlists(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM playlists ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Hängt einen Track ans Ende einer Playlist.
    pub fn add_to_playlist(&self, playlist_id: i64, track_id: i64) -> Result<()> {
        let next: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?1",
            params![playlist_id],
            |r| r.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO playlist_items (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
            params![playlist_id, track_id, next],
        )?;
        Ok(())
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<TrackRecord>> {
        let mut stmt = self.conn.prepare(&format!(
            "{SELECT_TRACK_PREFIXED} JOIN playlist_items pi ON pi.track_id = t.id \
             WHERE pi.playlist_id = ?1 ORDER BY pi.position"
        ))?;
        let rows = stmt.query_map(params![playlist_id], track_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

const SELECT_TRACK: &str = "SELECT id, path, fingerprint, title, artist, album, genre, \
                            duration_secs, bpm, beat_anchor_ms, musical_key, \
                            license, attribution, source FROM tracks";

const SELECT_TRACK_PREFIXED: &str = "SELECT t.id, t.path, t.fingerprint, t.title, t.artist, \
                                     t.album, t.genre, t.duration_secs, t.bpm, \
                                     t.beat_anchor_ms, t.musical_key, t.license, \
                                     t.attribution, t.source FROM tracks t";

fn track_from_row(row: &Row) -> rusqlite::Result<TrackRecord> {
    Ok(TrackRecord {
        id: row.get(0)?,
        path: row.get(1)?,
        fingerprint: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        album: row.get(5)?,
        genre: row.get(6)?,
        duration_secs: row.get(7)?,
        bpm: row.get(8)?,
        beat_anchor_ms: row.get(9)?,
        musical_key: row.get(10)?,
        license: row.get(11)?,
        attribution: row.get(12)?,
        source: Source::parse(&row.get::<_, String>(13)?),
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_core::Beatgrid;

    fn track(path: &str, artist: &str, title: &str, bpm: Option<f32>) -> TrackRecord {
        let mut t = TrackRecord::from_path(path);
        t.artist = Some(artist.into());
        t.title = Some(title.into());
        t.bpm = bpm;
        t
    }

    #[test]
    fn anlegen_und_wiederfinden() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert_track(&track("/a.mp3", "Rebekah", "Fear Paralysis", Some(135.0)))
            .unwrap();

        let geladen = lib.track(id).unwrap().expect("Track weg");
        assert_eq!(geladen.artist.as_deref(), Some("Rebekah"));
        assert_eq!(geladen.path, "/a.mp3");
        assert_eq!(lib.track_count().unwrap(), 1);
    }

    #[test]
    fn erneuter_scan_loescht_die_analyse_nicht() {
        // Der wichtigste Punkt am Upsert: Ein Ordner-Scan kennt nur Pfad und
        // Tags. Würde er die Analyse überschreiben, wäre sie nach jedem Scan
        // weg — und Analysieren ist die teure Arbeit.
        let lib = Library::open_in_memory().unwrap();

        let mut voll = track("/a.mp3", "Rebekah", "Fear Paralysis", Some(135.0));
        voll.set_beatgrid(Some(Beatgrid::new(135.0, 4_410, 1.0)), 44_100);
        voll.fingerprint = Some("abc123".into());
        lib.upsert_track(&voll).unwrap();

        // Zweiter Durchlauf ohne Analysedaten.
        let mager = TrackRecord::from_path("/a.mp3");
        lib.upsert_track(&mager).unwrap();

        let geladen = lib.track_by_path("/a.mp3").unwrap().unwrap();
        assert_eq!(geladen.bpm, Some(135.0), "BPM überschrieben");
        assert_eq!(geladen.fingerprint.as_deref(), Some("abc123"));
        assert_eq!(geladen.artist.as_deref(), Some("Rebekah"));
        assert_eq!(lib.track_count().unwrap(), 1, "Dublette angelegt");
    }

    #[test]
    fn suche_nach_text_trifft_titel_und_kuenstler() {
        let lib = Library::open_in_memory().unwrap();
        lib.upsert_track(&track("/a.mp3", "Rebekah", "Fear Paralysis", Some(135.0)))
            .unwrap();
        lib.upsert_track(&track("/b.mp3", "Len Faki", "My Black Sheep", Some(128.0)))
            .unwrap();

        assert_eq!(lib.search(&Query::text("rebekah")).unwrap().len(), 1);
        assert_eq!(lib.search(&Query::text("Sheep")).unwrap().len(), 1);
        assert_eq!(lib.search(&Query::text("zzz")).unwrap().len(), 0);
        assert_eq!(lib.search(&Query::default()).unwrap().len(), 2);
    }

    #[test]
    fn suche_nach_mixbarem_tempo() {
        let lib = Library::open_in_memory().unwrap();
        // 135 BPM läge bei ±6 % noch im Fenster — für diesen Test muss der
        // Ausreißer deutlich außerhalb liegen.
        lib.upsert_track(&track("/a.mp3", "A", "zu schnell", Some(150.0)))
            .unwrap();
        lib.upsert_track(&track("/b.mp3", "B", "passend", Some(129.0)))
            .unwrap();
        lib.upsert_track(&track("/c.mp3", "C", "ohne Tempo", None))
            .unwrap();

        let treffer = lib.search(&Query::mixable_with(128.0, 0.06)).unwrap();
        let titel: Vec<_> = treffer.iter().filter_map(|t| t.title.as_deref()).collect();

        assert_eq!(titel, vec!["passend"], "unerwartete Treffer: {titel:?}");
    }

    #[test]
    fn dubletten_werden_ueber_den_hash_gefunden() {
        let lib = Library::open_in_memory().unwrap();

        let mut a = track("/ordner1/x.mp3", "A", "X", Some(128.0));
        a.fingerprint = Some("gleich".into());
        let mut b = track("/ordner2/x.mp3", "A", "X", Some(128.0));
        b.fingerprint = Some("gleich".into());

        lib.upsert_track(&a).unwrap();
        lib.upsert_track(&b).unwrap();

        let dubletten = lib.tracks_by_fingerprint("gleich").unwrap();
        assert_eq!(dubletten.len(), 2, "derselbe Klang darf zweimal liegen");
    }

    #[test]
    fn fehlende_rechteangaben_lassen_sich_auflisten() {
        let lib = Library::open_in_memory().unwrap();

        let mut ohne = TrackRecord::from_path("/kick.wav");
        ohne.source = Source::Sample;
        lib.upsert_track(&ohne).unwrap();

        let mut mit = TrackRecord::from_path("/hat.wav");
        mit.source = Source::Sample;
        mit.license = Some("CC BY 4.0".into());
        mit.attribution = Some("jemand".into());
        lib.upsert_track(&mit).unwrap();

        lib.upsert_track(&TrackRecord::from_path("/eigen.mp3"))
            .unwrap();

        let luecken = lib.tracks_missing_attribution().unwrap();
        assert_eq!(luecken.len(), 1);
        assert_eq!(luecken[0].path, "/kick.wav");
    }

    #[test]
    fn cues_werden_ersetzt_nicht_angehaeuft() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib
            .upsert_track(&track("/a.mp3", "A", "X", Some(128.0)))
            .unwrap();

        let cue = |hot: u8, ms: f64| CueRecord {
            id: None,
            track_id: id,
            hotcue: Some(hot),
            position_ms: ms,
            name: None,
            kind: CueKind::Cue,
        };

        lib.replace_cues(id, &[cue(0, 1_000.0), cue(1, 2_000.0)])
            .unwrap();
        assert_eq!(lib.cues(id).unwrap().len(), 2);

        lib.replace_cues(id, &[cue(0, 500.0)]).unwrap();
        let cues = lib.cues(id).unwrap();
        assert_eq!(cues.len(), 1, "alte Cues blieben stehen");
        assert_eq!(cues[0].position_ms, 500.0);
        assert_eq!(cues[0].frame(48_000), 24_000);
    }

    #[test]
    fn playlists_behalten_ihre_reihenfolge() {
        let lib = Library::open_in_memory().unwrap();
        let a = lib
            .upsert_track(&track("/a.mp3", "A", "erst", None))
            .unwrap();
        let b = lib
            .upsert_track(&track("/b.mp3", "B", "dann", None))
            .unwrap();
        let c = lib
            .upsert_track(&track("/c.mp3", "C", "zuletzt", None))
            .unwrap();

        let pl = lib.create_playlist("Warmup").unwrap();
        lib.add_to_playlist(pl, c).unwrap();
        lib.add_to_playlist(pl, a).unwrap();
        lib.add_to_playlist(pl, b).unwrap();

        let titel: Vec<_> = lib
            .playlist_tracks(pl)
            .unwrap()
            .into_iter()
            .filter_map(|t| t.title)
            .collect();

        assert_eq!(titel, vec!["zuletzt", "erst", "dann"]);
        assert_eq!(lib.playlists().unwrap().len(), 1);
    }

    #[test]
    fn playlist_mit_gleichem_namen_wird_nicht_verdoppelt() {
        let lib = Library::open_in_memory().unwrap();
        let a = lib.create_playlist("Set").unwrap();
        let b = lib.create_playlist("Set").unwrap();

        assert_eq!(a, b);
        assert_eq!(lib.playlists().unwrap().len(), 1);
    }

    #[test]
    fn geloeschte_tracks_nehmen_ihre_cues_mit() {
        let lib = Library::open_in_memory().unwrap();
        let id = lib.upsert_track(&track("/a.mp3", "A", "X", None)).unwrap();
        lib.replace_cues(
            id,
            &[CueRecord {
                id: None,
                track_id: id,
                hotcue: Some(0),
                position_ms: 100.0,
                name: None,
                kind: CueKind::Cue,
            }],
        )
        .unwrap();

        lib.conn
            .execute("DELETE FROM tracks WHERE id = ?1", params![id])
            .unwrap();

        assert!(lib.cues(id).unwrap().is_empty(), "verwaiste Cues geblieben");
    }
}
