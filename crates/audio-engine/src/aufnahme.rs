//! Mitschnitt der Summe.
//!
//! Der Audio-Callback darf nicht auf die Platte schreiben. Ein `write` ist ein
//! Syscall, er kann Millisekunden dauern, und in dieser Zeit steht die
//! Wiedergabe — ein Aussetzer mitten im Set, und zwar genau dann, wenn man
//! mitschneidet.
//!
//! Also derselbe Weg wie beim AUX-Eingang, nur andersherum: ein lock-freier
//! Ringpuffer aus dem Audio-Thread heraus, ein eigener Thread, der ihn leert
//! und in eine WAV-Datei schreibt.
//!
//! **Wenn der Schreiber nicht hinterherkommt, gehen Frames verloren.** Das ist
//! die einzige Möglichkeit, die bleibt — blockieren wäre schlimmer. Verloren
//! heißt hier aber nicht verschwiegen: Sie werden gezählt und sind über
//! [`Aufnahme::verworfen`] abfragbar. Eine Aufnahme mit Lücken, die aussieht
//! wie eine ohne, wäre das schlechteste von allem.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use audio_core::track::CHANNELS;

/// Wie viel Vorrat der Ringpuffer fasst, in Sekunden.
///
/// Zwei Sekunden überbrücken jede normale Schreibpause. Größer hieße nur, dass
/// ein tatsächlich überlasteter Rechner den Verlust später merkt.
pub const PUFFER_SEKUNDEN: f32 = 2.0;

/// Der Griff, den der Audio-Thread hält.
///
/// Schreibt in den Ring und zählt, was nicht mehr hineinpasst. Allokiert nie.
pub struct Mitschnitt {
    tx: rtrb::Producer<f32>,
    aktiv: Arc<AtomicBool>,
    verworfen: Arc<AtomicU64>,
    geschrieben: Arc<AtomicU64>,
}

impl Mitschnitt {
    /// Nimmt einen Block verschränktes Stereo auf, falls die Aufnahme läuft.
    pub fn nimm_auf(&mut self, buffer: &[f32]) {
        if !self.aktiv.load(Ordering::Relaxed) {
            return;
        }

        let mut abgelegt = 0usize;
        for sample in buffer {
            if self.tx.push(*sample).is_err() {
                break;
            }
            abgelegt += 1;
        }

        let frames = (abgelegt / CHANNELS) as u64;
        self.geschrieben.fetch_add(frames, Ordering::Relaxed);

        let fehlend = buffer.len() - abgelegt;
        if fehlend > 0 {
            self.verworfen
                .fetch_add((fehlend / CHANNELS) as u64, Ordering::Relaxed);
        }
    }
}

enum Befehl {
    Start { pfad: PathBuf, sample_rate: u32 },
    Stop,
    Ende,
}

/// Der Griff für alle anderen: starten, stoppen, nachsehen.
pub struct Aufnahme {
    befehle: Sender<Befehl>,
    aktiv: Arc<AtomicBool>,
    verworfen: Arc<AtomicU64>,
    geschrieben: Arc<AtomicU64>,
    pfad: Option<PathBuf>,
    sample_rate: u32,
}

impl Aufnahme {
    pub fn starten(&mut self, pfad: &Path) -> Result<(), String> {
        if self.laeuft() {
            return Err(format!(
                "es läuft schon ein Mitschnitt nach {}",
                self.pfad
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            ));
        }

        // Höchstens **eine** Ebene anlegen. `~/sets/heute.wav` in einem noch
        // leeren Ordner soll gehen; ein Tippfehler soll dagegen nicht einen
        // ganzen Verzeichnisbaum irgendwo hinterlassen. Genau das ist beim
        // Testen passiert — als root legte `create_dir_all` klaglos vier
        // Ebenen unter `/` an.
        if let Some(ordner) = pfad.parent().filter(|o| !o.as_os_str().is_empty()) {
            if !ordner.is_dir() {
                let da = ordner
                    .parent()
                    .is_none_or(|o| o.as_os_str().is_empty() || o.is_dir());
                if !da {
                    return Err(format!("{} gibt es nicht", ordner.display()));
                }
                std::fs::create_dir(ordner).map_err(|e| format!("{}: {e}", ordner.display()))?;
            }
        }

        // Erst die Datei öffnen lassen, dann den Audio-Thread losschreiben —
        // andersherum liefen die ersten Frames ins Leere.
        // Probehalber anlegen: Ein unschreibbarer Pfad soll sofort auffallen
        // und nicht erst nach dem Set.
        File::create(pfad).map_err(|e| format!("{}: {e}", pfad.display()))?;

        self.verworfen.store(0, Ordering::Relaxed);
        self.geschrieben.store(0, Ordering::Relaxed);
        self.befehle
            .send(Befehl::Start {
                pfad: pfad.to_path_buf(),
                sample_rate: self.sample_rate,
            })
            .map_err(|_| "der Schreiber läuft nicht mehr".to_string())?;

        self.pfad = Some(pfad.to_path_buf());
        self.aktiv.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Beendet den Mitschnitt und gibt zurück, wohin er ging.
    pub fn stoppen(&mut self) -> Option<PathBuf> {
        if !self.laeuft() {
            return None;
        }

        // Erst der Audio-Thread, dann der Schreiber: Was noch im Ring liegt,
        // soll er zu Ende schreiben.
        self.aktiv.store(false, Ordering::Relaxed);
        let _ = self.befehle.send(Befehl::Stop);
        self.pfad.take()
    }

    pub fn laeuft(&self) -> bool {
        self.aktiv.load(Ordering::Relaxed)
    }

    pub fn pfad(&self) -> Option<&Path> {
        self.pfad.as_deref()
    }

    /// Aufgenommene Frames — durch die Samplerate geteilt ergibt das Sekunden.
    pub fn frames(&self) -> u64 {
        self.geschrieben.load(Ordering::Relaxed)
    }

    pub fn sekunden(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }

    /// Mit welcher Rate mitgeschnitten wird.
    ///
    /// Wer Frames aufhebt, um sie später einer Stelle im Mitschnitt zuzuordnen,
    /// braucht sie dazu — sonst sind es Zahlen ohne Zeit.
    pub fn rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames, die nicht mehr in den Ring passten.
    ///
    /// Alles über null heißt: Der Mitschnitt hat Lücken.
    pub fn verworfen(&self) -> u64 {
        self.verworfen.load(Ordering::Relaxed)
    }
}

impl Drop for Aufnahme {
    fn drop(&mut self) {
        self.aktiv.store(false, Ordering::Relaxed);
        let _ = self.befehle.send(Befehl::Ende);
    }
}

/// Legt Ringpuffer und Schreiber-Thread an.
pub fn mitschnitt(sample_rate: u32) -> (Mitschnitt, Aufnahme) {
    let kapazitaet = (sample_rate as f32 * PUFFER_SEKUNDEN) as usize * CHANNELS;
    let (tx, rx) = rtrb::RingBuffer::new(kapazitaet);

    let aktiv = Arc::new(AtomicBool::new(false));
    let verworfen = Arc::new(AtomicU64::new(0));
    let geschrieben = Arc::new(AtomicU64::new(0));
    let (befehle, eingang) = std::sync::mpsc::channel();

    std::thread::Builder::new()
        .name("mitschnitt".into())
        .spawn(move || schreiben(rx, eingang))
        .expect("Schreiber-Thread ließ sich nicht starten");

    (
        Mitschnitt {
            tx,
            aktiv: Arc::clone(&aktiv),
            verworfen: Arc::clone(&verworfen),
            geschrieben: Arc::clone(&geschrieben),
        },
        Aufnahme {
            befehle,
            aktiv,
            verworfen,
            geschrieben,
            pfad: None,
            sample_rate,
        },
    )
}

/// Der Schreiber-Thread.
///
/// Leert den Ring **immer**, auch ohne offene Datei. Täte er das nicht, liefe
/// der Puffer nach einem Stopp voll, und der nächste Start begänne mit den
/// Resten des letzten.
///
/// Beim `Start` wird dagegen **nicht** geleert, und das ist wichtiger, als es
/// aussieht. Der Aufnehmende schreibt in den Ring, sobald `starten` zurück ist;
/// dieser Thread bekommt den Befehl erst, wenn er das nächste Mal an der Reihe
/// ist. Auf einem ausgelasteten Rechner liegen dann schon Samples im Ring, und
/// ein Leeren an dieser Stelle würfe genau den Anfang des Mitschnitts weg.
/// Nötig ist es auch nicht: Nach dem Schließen ist der Ring leer, und außerhalb
/// einer laufenden Aufnahme legt niemand etwas hinein.
fn schreiben(mut rx: rtrb::Consumer<f32>, befehle: Receiver<Befehl>) {
    const TAKT: std::time::Duration = std::time::Duration::from_millis(20);
    let mut datei: Option<WavDatei> = None;
    let mut block: Vec<f32> = Vec::with_capacity(8_192);

    loop {
        let mut beenden = false;
        let mut schliessen = false;

        while let Ok(befehl) = befehle.try_recv() {
            match befehl {
                Befehl::Start { pfad, sample_rate } => {
                    match WavDatei::anlegen(&pfad, sample_rate) {
                        Ok(d) => datei = Some(d),
                        Err(e) => eprintln!("Mitschnitt: {e}"),
                    }
                }
                Befehl::Stop => schliessen = true,
                Befehl::Ende => {
                    schliessen = true;
                    beenden = true;
                }
            }
        }

        block.clear();
        while let Ok(sample) = rx.pop() {
            block.push(sample);
            if block.len() >= 8_192 {
                break;
            }
        }

        if let Some(d) = datei.as_mut() {
            if !block.is_empty() {
                if let Err(e) = d.schreiben(&block) {
                    eprintln!("Mitschnitt: {e}");
                }
            }
        }

        if schliessen {
            // Was noch im Ring liegt, gehört noch in die Datei.
            block.clear();
            while let Ok(sample) = rx.pop() {
                block.push(sample);
            }
            if let Some(mut d) = datei.take() {
                if !block.is_empty() {
                    let _ = d.schreiben(&block);
                }
                if let Err(e) = d.abschliessen() {
                    eprintln!("Mitschnitt: {e}");
                }
            }
            // Ab hier ist der Ring leer — darauf verlässt sich der nächste
            // Start, statt selbst zu leeren.
            while rx.pop().is_ok() {}
        }

        if beenden {
            return;
        }
        if block.is_empty() {
            std::thread::sleep(TAKT);
        }
    }
}

/// Eine WAV-Datei, die wächst.
///
/// Der Kopf einer WAV-Datei enthält die Länge, die man beim Anlegen noch nicht
/// kennt. Also erst Platzhalter schreiben und am Ende zurückspringen — der
/// übliche Weg, und der Grund, warum eine abgebrochene Aufnahme einen falschen
/// Kopf hat und von manchen Programmen nicht gelesen wird.
struct WavDatei {
    schreiber: BufWriter<File>,
    daten_bytes: u32,
}

const KOPF_BYTES: u32 = 44;

impl WavDatei {
    fn anlegen(pfad: &Path, sample_rate: u32) -> std::io::Result<WavDatei> {
        let mut schreiber = BufWriter::new(File::create(pfad)?);
        let kanaele = CHANNELS as u16;
        let bits = 16u16;
        let block = kanaele * bits / 8;
        let byte_rate = sample_rate * block as u32;

        schreiber.write_all(b"RIFF")?;
        schreiber.write_all(&0u32.to_le_bytes())?; // später
        schreiber.write_all(b"WAVE")?;
        schreiber.write_all(b"fmt ")?;
        schreiber.write_all(&16u32.to_le_bytes())?;
        schreiber.write_all(&1u16.to_le_bytes())?; // PCM
        schreiber.write_all(&kanaele.to_le_bytes())?;
        schreiber.write_all(&sample_rate.to_le_bytes())?;
        schreiber.write_all(&byte_rate.to_le_bytes())?;
        schreiber.write_all(&block.to_le_bytes())?;
        schreiber.write_all(&bits.to_le_bytes())?;
        schreiber.write_all(b"data")?;
        schreiber.write_all(&0u32.to_le_bytes())?; // später

        Ok(WavDatei {
            schreiber,
            daten_bytes: 0,
        })
    }

    fn schreiben(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for sample in samples {
            // Runden statt abschneiden: Abschneiden verschiebt jedes Sample in
            // dieselbe Richtung und klingt als leise Verzerrung mit.
            let wert = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
            self.schreiber.write_all(&wert.to_le_bytes())?;
        }
        self.daten_bytes += (samples.len() * 2) as u32;
        Ok(())
    }

    fn abschliessen(&mut self) -> std::io::Result<()> {
        self.schreiber.flush()?;
        let datei = self.schreiber.get_mut();

        datei.seek(SeekFrom::Start(4))?;
        datei.write_all(&(KOPF_BYTES - 8 + self.daten_bytes).to_le_bytes())?;
        datei.seek(SeekFrom::Start(40))?;
        datei.write_all(&self.daten_bytes.to_le_bytes())?;
        datei.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn scratch(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("musik-aufnahme-{}-{name}.wav", std::process::id()));
        p
    }

    fn warte_bis(bedingung: impl Fn() -> bool) -> bool {
        for _ in 0..200 {
            if bedingung() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn ein_mitschnitt_landet_als_lesbare_wav_datei() {
        let pfad = scratch("grund");
        let (mut tap, mut aufnahme) = mitschnitt(RATE);

        aufnahme.starten(&pfad).expect("Start");
        assert!(aufnahme.laeuft());

        // Eine halbe Sekunde Vollausschlag, damit man das Ergebnis messen kann.
        let block: Vec<f32> = (0..RATE as usize / 2)
            .flat_map(|_| [0.5f32, -0.5])
            .collect();
        tap.nimm_auf(&block);

        assert!(warte_bis(|| aufnahme.frames() > 0), "es kommt nichts an");
        aufnahme.stoppen();
        assert!(!aufnahme.laeuft());

        assert!(
            warte_bis(|| std::fs::metadata(&pfad)
                .map(|m| m.len() > KOPF_BYTES as u64)
                .unwrap_or(false)),
            "die Datei bleibt leer"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));

        let daten = std::fs::read(&pfad).expect("lesen");
        assert_eq!(&daten[0..4], b"RIFF");
        assert_eq!(&daten[8..12], b"WAVE");

        // Der Kopf muss die wirkliche Länge nennen, sonst lesen manche
        // Programme die Datei gar nicht.
        let daten_bytes = u32::from_le_bytes(daten[40..44].try_into().unwrap()) as usize;
        assert_eq!(
            daten_bytes,
            daten.len() - KOPF_BYTES as usize,
            "die Länge im Kopf passt nicht zur Datei"
        );
        assert!(daten_bytes > 0);

        // Und der Inhalt ist das, was hineinging: ±0.5 als 16 Bit.
        let erstes = i16::from_le_bytes(daten[44..46].try_into().unwrap());
        assert!(
            (erstes as f32 / i16::MAX as f32 - 0.5).abs() < 0.001,
            "erstes Sample ist {erstes}"
        );

        let _ = std::fs::remove_file(&pfad);
    }

    /// Der Fall, der auf einem ausgelasteten Läufer schiefging.
    ///
    /// Ohne Pause zwischen Start, Aufnehmen und Stopp bekommt der Schreiber
    /// alle drei auf einmal. Leerte er dabei den Ring — was er tat, um Reste
    /// loszuwerden —, landete kein einziges Sample in der Datei, und heraus
    /// kam ein WAV mit nichts als einem Kopf.
    #[test]
    fn ein_sofortiger_stopp_verliert_den_anfang_nicht() {
        let pfad = scratch("sofort");
        let (mut tap, mut aufnahme) = mitschnitt(RATE);

        aufnahme.starten(&pfad).expect("Start");
        let block: Vec<f32> = (0..1_000).flat_map(|_| [0.5f32, -0.5]).collect();
        tap.nimm_auf(&block);
        aufnahme.stoppen();

        let erwartet = KOPF_BYTES as u64 + block.len() as u64 * 2;
        assert!(
            warte_bis(|| std::fs::metadata(&pfad)
                .map(|m| m.len() == erwartet)
                .unwrap_or(false)),
            "die Datei hat {:?} statt {erwartet} Bytes",
            std::fs::metadata(&pfad).map(|m| m.len())
        );

        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn ein_zweiter_mitschnitt_erbt_nichts_vom_ersten() {
        let erster = scratch("erbe1");
        let zweiter = scratch("erbe2");
        let (mut tap, mut aufnahme) = mitschnitt(RATE);

        aufnahme.starten(&erster).unwrap();
        tap.nimm_auf(&vec![0.5; 2_000]);
        aufnahme.stoppen();
        assert!(warte_bis(|| std::fs::metadata(&erster)
            .map(|m| m.len() > KOPF_BYTES as u64)
            .unwrap_or(false)));

        // Zwischen den Aufnahmen schreibt niemand — was jetzt käme, wäre ein
        // Rest, und der gehört nicht in den zweiten Mitschnitt.
        let block: Vec<f32> = (0..500).flat_map(|_| [0.25f32, -0.25]).collect();
        aufnahme.starten(&zweiter).unwrap();
        tap.nimm_auf(&block);
        aufnahme.stoppen();

        let erwartet = KOPF_BYTES as u64 + block.len() as u64 * 2;
        assert!(
            warte_bis(|| std::fs::metadata(&zweiter)
                .map(|m| m.len() == erwartet)
                .unwrap_or(false)),
            "der zweite hat {:?} statt {erwartet} Bytes",
            std::fs::metadata(&zweiter).map(|m| m.len())
        );

        let _ = std::fs::remove_file(&erster);
        let _ = std::fs::remove_file(&zweiter);
    }

    #[test]
    fn ohne_laufende_aufnahme_wird_nichts_geschrieben() {
        let (mut tap, aufnahme) = mitschnitt(RATE);
        tap.nimm_auf(&vec![0.5; 4_096]);
        assert_eq!(aufnahme.frames(), 0);
        assert_eq!(aufnahme.verworfen(), 0);
    }

    #[test]
    fn ein_zweiter_start_wird_abgewiesen_statt_den_ersten_zu_verlieren() {
        let pfad = scratch("doppelt");
        let (_tap, mut aufnahme) = mitschnitt(RATE);

        aufnahme.starten(&pfad).unwrap();
        let zweiter = aufnahme.starten(&scratch("doppelt2"));
        assert!(zweiter.is_err(), "der zweite Start überschreibt den ersten");

        aufnahme.stoppen();
        let _ = std::fs::remove_file(&pfad);
    }

    #[test]
    fn ein_unschreibbarer_pfad_faellt_sofort_auf() {
        let (_tap, mut aufnahme) = mitschnitt(RATE);
        let fehler = aufnahme.starten(Path::new("/gibt/es/nicht/und/geht/nicht.wav"));
        assert!(fehler.is_err(), "{fehler:?}");
        assert!(!aufnahme.laeuft(), "eine gescheiterte Aufnahme läuft nicht");
        assert!(
            !Path::new("/gibt").exists(),
            "ein Tippfehler hat einen Verzeichnisbaum hinterlassen"
        );
    }

    #[test]
    fn ein_fehlender_ordner_wird_angelegt_aber_nur_einer() {
        let mut ordner = std::env::temp_dir();
        ordner.push(format!("musik-neu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ordner);

        let (_tap, mut aufnahme) = mitschnitt(RATE);
        assert!(aufnahme.starten(&ordner.join("set.wav")).is_ok());
        aufnahme.stoppen();

        let _ = std::fs::remove_dir_all(&ordner);
    }

    #[test]
    fn stoppen_ohne_aufnahme_ist_kein_fehler() {
        let (_tap, mut aufnahme) = mitschnitt(RATE);
        assert_eq!(aufnahme.stoppen(), None);
    }

    /// Verlorene Frames werden gezählt, nicht verschwiegen.
    ///
    /// Ein Mitschnitt mit Lücken, der aussieht wie einer ohne, wäre das
    /// schlechteste Ergebnis von allen.
    #[test]
    fn was_nicht_mehr_hineinpasst_wird_gezaehlt() {
        // Ein winziger Ring ohne Leser — so lässt sich der Überlauf
        // herbeiführen, ohne auf eine überlastete Platte zu warten.
        let (tx, _rx) = rtrb::RingBuffer::<f32>::new(200);
        let aktiv = Arc::new(AtomicBool::new(true));
        let verworfen = Arc::new(AtomicU64::new(0));
        let geschrieben = Arc::new(AtomicU64::new(0));

        let mut tap = Mitschnitt {
            tx,
            aktiv,
            verworfen: Arc::clone(&verworfen),
            geschrieben: Arc::clone(&geschrieben),
        };

        tap.nimm_auf(&vec![0.25; 1_000]);

        assert_eq!(geschrieben.load(Ordering::Relaxed), 100);
        assert_eq!(verworfen.load(Ordering::Relaxed), 400);
    }
}
