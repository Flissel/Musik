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
    Start {
        pfad: PathBuf,
        sample_rate: u32,
    },
    /// Schluss — und wie viele Frames noch in diese Datei gehören.
    ///
    /// Die Zahl kommt von der Seite, die aufnimmt, und ist die einzige, mit der
    /// sich die Grenze zwischen zwei Mitschnitten bestimmen lässt. Ohne sie
    /// müsste der Schreiber „alles, was noch im Ring liegt" nehmen — und darin
    /// steckt womöglich schon der Anfang des nächsten.
    Stop {
        frames: u64,
    },
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
        let _ = self.befehle.send(Befehl::Stop {
            frames: self.geschrieben.load(Ordering::Relaxed),
        });
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
    let (tap, aufnahme, _nummer) = mitschnitt_intern(sample_rate);
    (tap, aufnahme)
}

/// Wie [`mitschnitt`], gibt aber die Nummer des Schreibers mit heraus. Nur der
/// Test braucht sie — um genau diesen einen Schreiber anzuhalten.
fn mitschnitt_intern(sample_rate: u32) -> (Mitschnitt, Aufnahme, u64) {
    let kapazitaet = (sample_rate as f32 * PUFFER_SEKUNDEN) as usize * CHANNELS;
    let (tx, rx) = rtrb::RingBuffer::new(kapazitaet);

    let aktiv = Arc::new(AtomicBool::new(false));
    let verworfen = Arc::new(AtomicU64::new(0));
    let geschrieben = Arc::new(AtomicU64::new(0));
    let (befehle, eingang) = std::sync::mpsc::channel();

    let id = schreiber_id();
    let mitzaehlen = Arc::clone(&geschrieben);
    std::thread::Builder::new()
        .name("mitschnitt".into())
        .spawn(move || schreiben(rx, eingang, mitzaehlen, id))
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
        id,
    )
}

/// Der Schreiber-Thread.
///
/// Leert den Ring **immer**, auch ohne offene Datei. Täte er das nicht, liefe
/// der Puffer nach einem Stopp voll, und der nächste Start begänne mit den
/// Resten des letzten. Er leert ihn aber nur so weit, wie sich die Frames
/// zuordnen lassen — dazu unten beim Deckel mehr.
///
/// Beim `Start` wird dagegen **nicht** geleert, und das ist wichtiger, als es
/// aussieht. Der Aufnehmende schreibt in den Ring, sobald `starten` zurück ist;
/// dieser Thread bekommt den Befehl erst, wenn er das nächste Mal an der Reihe
/// ist. Auf einem ausgelasteten Rechner liegen dann schon Samples im Ring, und
/// ein Leeren an dieser Stelle würfe genau den Anfang des Mitschnitts weg.
///
/// # Befehle werden der Reihe nach ausgeführt
///
/// Er holt sich seine Befehle in Häppchen ab, und in einem Häppchen können
/// `Stop` und der nächste `Start` zusammen liegen — zwischen zwei Stücken tut
/// das jeder. Sie zu Merkern zusammenzufassen und erst danach zu handeln war
/// falsch: Dann öffnete der `Start` die neue Datei, und der `Stop` schrieb den
/// *alten* Mitschnitt hinein und schloss sie. Die erste Datei blieb bei 44
/// Bytes, die zweite bekam fremdes Material, und der zweite Mitschnitt landete
/// nirgends.
///
/// Aufgefallen ist das nicht hier, sondern in der CI: Auf einem freien Rechner
/// kommt der Schreiber zwischen Stopp und Start dran, auf einem ausgelasteten
/// nicht.
///
/// # Wo ein Mitschnitt aufhört
///
/// Der `Stop` bringt die Zahl der Frames mit, die die aufnehmende Seite
/// abgelegt hat. Der Schreiber füllt bis dorthin und lässt liegen, was danach
/// kommt — das gehört schon zum nächsten. „Alles, was noch im Ring ist" wäre
/// die naheliegende Regel und die falsche: Genau darin steckt der Anfang des
/// nächsten Mitschnitts.
/// Ein Haltepunkt im Schreiber, den nur der Test kennt.
///
/// Die Lücke, um die es geht, ist ein paar Maschinenbefehle breit — zwischen
/// „Befehle abholen" und „Ring leeren". Sie durch Wiederholen zu treffen ging
/// nicht: vierzig Läufe mit zwanzig Runden blieben grün. Also wird der
/// Schreiber an genau dieser Stelle angehalten, während der Test in Ruhe
/// stoppt, neu startet und nachlegt.
///
/// Außerhalb von `cfg(test)` bleibt davon nichts übrig — beide Funktionen sind
/// leer und werden wegoptimiert.
#[cfg(not(test))]
#[inline(always)]
fn schreiber_id() -> u64 {
    0
}

#[cfg(not(test))]
#[inline(always)]
fn haltepunkt(_id: u64) {}

#[cfg(test)]
static NAECHSTE_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
static ANGEHALTEN: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
static PARKT: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
fn schreiber_id() -> u64 {
    NAECHSTE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Hält **nur** den Schreiber an, dessen Nummer der Test angemeldet hat. Alle
/// anderen laufen weiter — die Tests dieser Datei laufen nebeneinander.
#[cfg(test)]
fn haltepunkt(id: u64) {
    if ANGEHALTEN.load(Ordering::Acquire) != id {
        return;
    }
    PARKT.store(id, Ordering::Release);
    while ANGEHALTEN.load(Ordering::Acquire) == id {
        std::hint::spin_loop();
    }
    PARKT.store(0, Ordering::Release);
}

fn schreiben(
    mut rx: rtrb::Consumer<f32>,
    befehle: Receiver<Befehl>,
    geschrieben: Arc<AtomicU64>,
    id: u64,
) {
    const TAKT: std::time::Duration = std::time::Duration::from_millis(20);
    let mut datei: Option<WavDatei> = None;
    let mut frames: u64 = 0;
    let mut block: Vec<f32> = Vec::with_capacity(8_192);

    /// Holt bis zu `hoechstens` Frames aus dem Ring.
    fn holen(rx: &mut rtrb::Consumer<f32>, block: &mut Vec<f32>, hoechstens: u64) {
        block.clear();
        let grenze = hoechstens.saturating_mul(CHANNELS as u64).min(8_192) as usize;
        while block.len() < grenze {
            match rx.pop() {
                Ok(sample) => block.push(sample),
                Err(_) => break,
            }
        }
    }

    loop {
        let mut beenden = false;
        // Der Stand **vor** dem Abholen der Befehle. Alles, was danach in den
        // Ring kommt, bleibt bis zur nächsten Runde liegen — siehe unten.
        let stand = geschrieben.load(Ordering::Relaxed);
        let mut frisch_gestartet = false;

        while let Ok(befehl) = befehle.try_recv() {
            match befehl {
                Befehl::Start { pfad, sample_rate } => {
                    // Eine noch offene Datei zuerst ordentlich schließen. Ohne
                    // das bliebe ihr Kopf ungeschrieben, und eine WAV-Datei mit
                    // falschem Kopf lesen manche Programme gar nicht.
                    if let Some(mut d) = datei.take() {
                        let _ = d.abschliessen();
                    }
                    frames = 0;
                    frisch_gestartet = true;
                    match WavDatei::anlegen(&pfad, sample_rate) {
                        Ok(d) => datei = Some(d),
                        Err(e) => eprintln!("Mitschnitt: {e}"),
                    }
                }
                Befehl::Stop { frames: soll } => {
                    abschliessen(&mut rx, &mut datei, &mut block, soll.saturating_sub(frames));
                    frames = 0;
                }
                Befehl::Ende => {
                    // Beim Beenden gibt es kein Danach — alles, was noch da
                    // ist, gehört in diese Datei.
                    abschliessen(&mut rx, &mut datei, &mut block, u64::MAX);
                    beenden = true;
                }
            }
        }

        haltepunkt(id);

        // **Nur so weit leeren, wie es sich zuordnen lässt.**
        //
        // `stand` wurde abgelesen, bevor die Befehle drankamen. Ein Frame, der
        // darin mitzählt, wurde also vorher abgelegt — und wenn er zu einem
        // *neueren* Mitschnitt gehört, dann wurde dessen `Start` ebenfalls
        // vorher abgeschickt und ist in dieser Runde schon abgeholt. Anders
        // gesagt: Was hier gezählt wird, gehört nie zu einer Datei, die noch
        // kommt.
        //
        // Nach einem frischen `Start` gilt der Stand nicht mehr — er zählt
        // dann womöglich noch den vorigen Mitschnitt. Eine Runde warten kostet
        // eine Zwanzigstelsekunde und liegt weit innerhalb des Vorrats.
        let deckel = if frisch_gestartet {
            0
        } else {
            stand.saturating_sub(frames)
        };
        holen(&mut rx, &mut block, deckel);
        if let Some(d) = datei.as_mut() {
            if !block.is_empty() {
                if let Err(e) = d.schreiben(&block) {
                    eprintln!("Mitschnitt: {e}");
                }
                frames += (block.len() / CHANNELS) as u64;
            }
        }

        if beenden {
            return;
        }
        if block.is_empty() {
            std::thread::sleep(TAKT);
        }
    }
}

/// Schreibt den Rest eines Mitschnitts und schließt die Datei.
///
/// `rest` ist, wie viele Frames noch fehlen. Alles darüber bleibt im Ring: Es
/// gehört zum nächsten Mitschnitt.
fn abschliessen(
    rx: &mut rtrb::Consumer<f32>,
    datei: &mut Option<WavDatei>,
    block: &mut Vec<f32>,
    rest: u64,
) {
    let Some(mut d) = datei.take() else {
        return;
    };
    let mut offen = rest;
    while offen > 0 {
        block.clear();
        let grenze = offen.saturating_mul(CHANNELS as u64).min(8_192) as usize;
        while block.len() < grenze {
            match rx.pop() {
                Ok(sample) => block.push(sample),
                Err(_) => break,
            }
        }
        if block.is_empty() {
            break;
        }
        if let Err(e) = d.schreiben(block) {
            eprintln!("Mitschnitt: {e}");
        }
        offen = offen.saturating_sub((block.len() / CHANNELS) as u64);
    }
    if let Err(e) = d.abschliessen() {
        eprintln!("Mitschnitt: {e}");
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

    /// **Zwanzig Mitschnitte hintereinander, ohne einmal Luft zu holen.**
    ///
    /// Zwanzig `Start`/`Stop`-Paare liegen dann auf einen Schlag im Briefkasten
    /// und alle Frames zusammen im Ring; der Schreiber muss sie allein aus den
    /// Zahlen im `Stop` auseinandersortieren. Verschiedene Längen sorgen dafür,
    /// dass ein Verrutschen um einen einzigen Mitschnitt auffällt.
    ///
    /// **Den Fehler aus der CI trifft dieser Test nicht** — vierzig Läufe
    /// blieben grün, auch mit dem Fehler drin. Dafür ist die Lücke zu schmal;
    /// das leistet erst `der_ring_wird_nur_bis_zur_grenze_geleert` mit seinem
    /// Haltepunkt. Hier bleibt eine billige Grundprüfung.
    #[test]
    fn viele_mitschnitte_hintereinander_verrutschen_nicht() {
        const RUNDEN: usize = 20;
        let (mut tap, mut aufnahme) = mitschnitt(RATE);

        // Verschiedene Längen: Rutscht etwas, passt keine einzige Größe mehr.
        let laengen: Vec<usize> = (0..RUNDEN).map(|r| 200 + r * 13).collect();
        let dateien: Vec<PathBuf> = (0..RUNDEN).map(|r| scratch(&format!("hetze{r}"))).collect();

        for (r, pfad) in dateien.iter().enumerate() {
            let block: Vec<f32> = (0..laengen[r]).flat_map(|_| [0.5f32, -0.5]).collect();
            aufnahme.starten(pfad).unwrap();
            tap.nimm_auf(&block);
            aufnahme.stoppen();
        }

        for (r, pfad) in dateien.iter().enumerate() {
            let soll = KOPF_BYTES as u64 + laengen[r] as u64 * CHANNELS as u64 * 2;
            assert!(
                warte_bis(|| std::fs::metadata(pfad)
                    .map(|m| m.len() == soll)
                    .unwrap_or(false)),
                "Runde {r} hat {:?} statt {soll} Bytes",
                std::fs::metadata(pfad).map(|m| m.len())
            );
        }

        for pfad in &dateien {
            let _ = std::fs::remove_file(pfad);
        }
    }

    /// **Der Ring wird nur so weit geleert, wie es sich zuordnen lässt.**
    ///
    /// Der Fehler dahinter ist einmal in der CI aufgetaucht und danach nie
    /// wieder: Die erste Datei hatte 5244 statt 4044 Bytes — also 1300 Frames
    /// statt 1000. Genau 1000 + 300, die beiden Mitschnitte in einer Datei.
    ///
    /// Der Weg dorthin ließ sich lesen, aber nicht treffen. Der Schreiber holt
    /// erst die Befehle ab und leert dann den Ring; wer in dieser Lücke stoppt,
    /// neu startet und nachlegt, dessen zweiter Mitschnitt liegt schon im Ring,
    /// während der `Stop` noch im Briefkasten steckt. Vierzig Läufe mit zwanzig
    /// Runden blieben grün — die Lücke ist ein paar Maschinenbefehle breit.
    ///
    /// Also wird der Schreiber angehalten. Zweimal: einmal, damit er den
    /// `Start` überhaupt verarbeitet und die erste Datei offen ist, und dann
    /// noch einmal in genau der Lücke. Das zweite Parken kann erst in einer
    /// Runde passieren, die den `Start` schon abgeholt hat — die Runde davor
    /// hat ihren Haltepunkt bereits hinter sich.
    ///
    /// Dieser Test ist der einzige, der den [`haltepunkt`] benutzt, und der
    /// Grund, dass es ihn gibt.
    #[test]
    fn der_ring_wird_nur_bis_zur_grenze_geleert() {
        let erster = scratch("grenze1");
        let zweiter = scratch("grenze2");
        let (mut tap, mut aufnahme, nummer) = mitschnitt_intern(RATE);

        let eins: Vec<f32> = (0..1_000).flat_map(|_| [0.5f32, -0.5]).collect();
        let zwei: Vec<f32> = (0..300).flat_map(|_| [0.25f32, -0.25]).collect();

        let parken = |nummer: u64| {
            ANGEHALTEN.store(nummer, Ordering::Release);
            assert!(
                warte_bis(|| PARKT.load(Ordering::Acquire) == nummer),
                "der Schreiber ist nicht stehen geblieben"
            );
        };
        // Losfahren lassen und **abwarten, bis er wirklich weg ist**. Ohne das
        // Abwarten wurde beim zweiten Mal gar nicht neu geparkt: Der Schreiber
        // sah die Null nie, blieb stehen, wo er stand, und der Test hielt das
        // alte Parken für das neue. Er prüfte dann etwas anderes als gedacht.
        let weiterfahren = |nummer: u64| {
            ANGEHALTEN.store(0, Ordering::Release);
            assert!(
                warte_bis(|| PARKT.load(Ordering::Acquire) != nummer),
                "der Schreiber ist nicht weitergefahren"
            );
        };

        // Erstes Parken: den `Start` losschicken, während er steht. Er holt ihn
        // in einer der nächsten Runden ab und legt die Datei an.
        parken(nummer);
        aufnahme.starten(&erster).unwrap();
        weiterfahren(nummer);

        // Zweites Parken: jetzt ist die erste Datei offen, und der Ring ist
        // leer. Genau hier saß der Fehler.
        parken(nummer);
        tap.nimm_auf(&eins);
        aufnahme.stoppen();
        aufnahme.starten(&zweiter).unwrap();
        tap.nimm_auf(&zwei);
        weiterfahren(nummer);

        let soll_eins = KOPF_BYTES as u64 + eins.len() as u64 * 2;
        let soll_zwei = KOPF_BYTES as u64 + zwei.len() as u64 * 2;
        assert!(
            warte_bis(|| std::fs::metadata(&erster)
                .map(|m| m.len() == soll_eins)
                .unwrap_or(false)),
            "der erste hat {:?} statt {soll_eins} Bytes — hat er den zweiten mitgenommen?",
            std::fs::metadata(&erster).map(|m| m.len())
        );
        aufnahme.stoppen();
        assert!(
            warte_bis(|| std::fs::metadata(&zweiter)
                .map(|m| m.len() == soll_zwei)
                .unwrap_or(false)),
            "der zweite hat {:?} statt {soll_zwei} Bytes",
            std::fs::metadata(&zweiter).map(|m| m.len())
        );

        let _ = std::fs::remove_file(&erster);
        let _ = std::fs::remove_file(&zweiter);
    }

    /// **Stopp und Start ohne Pause dazwischen.**
    ///
    /// Zwischen zwei Stücken tut das jeder: `record_stop`, dann sofort
    /// `record naechstes.wav`. Der Schreiber holt sich seine Befehle in
    /// Häppchen ab, und wenn beide im selben Häppchen liegen, muss er sie
    /// trotzdem der Reihe nach ausführen — sonst landet der erste Mitschnitt
    /// in der zweiten Datei und der zweite nirgends.
    ///
    /// Der Test wartet ausdrücklich **nicht** zwischen den Aufnahmen. Genau
    /// dieses Warten hat den Fehler bisher versteckt: Auf einem freien Rechner
    /// kommt der Schreiber zwischendurch dran, auf einem ausgelasteten nicht.
    /// Aufgefallen ist er auch nicht hier, sondern in der CI.
    #[test]
    fn stopp_und_start_ohne_pause_landen_in_zwei_dateien() {
        let erster = scratch("hektik1");
        let zweiter = scratch("hektik2");
        let (mut tap, mut aufnahme) = mitschnitt(RATE);

        let eins: Vec<f32> = (0..1_000).flat_map(|_| [0.5f32, -0.5]).collect();
        let zwei: Vec<f32> = (0..300).flat_map(|_| [0.25f32, -0.25]).collect();

        aufnahme.starten(&erster).unwrap();
        tap.nimm_auf(&eins);
        aufnahme.stoppen();
        // Kein Warten — das ist der Punkt.
        aufnahme.starten(&zweiter).unwrap();
        tap.nimm_auf(&zwei);
        aufnahme.stoppen();

        let soll_eins = KOPF_BYTES as u64 + eins.len() as u64 * 2;
        let soll_zwei = KOPF_BYTES as u64 + zwei.len() as u64 * 2;
        assert!(
            warte_bis(|| std::fs::metadata(&erster)
                .map(|m| m.len() == soll_eins)
                .unwrap_or(false)),
            "der erste hat {:?} statt {soll_eins} Bytes",
            std::fs::metadata(&erster).map(|m| m.len())
        );
        assert!(
            warte_bis(|| std::fs::metadata(&zweiter)
                .map(|m| m.len() == soll_zwei)
                .unwrap_or(false)),
            "der zweite hat {:?} statt {soll_zwei} Bytes",
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
