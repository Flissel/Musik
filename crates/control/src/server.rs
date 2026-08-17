//! Die Steckdose: das Protokoll über einen Unix-Socket.
//!
//! Ein Socket im Dateisystem und nicht TCP, und das ist eine
//! Sicherheitsentscheidung, keine Bequemlichkeit. Wer hier hineinschreibt,
//! steuert die Anlage. Ein offener TCP-Port täte das für jeden im selben
//! Netz — auf einer Clubbühne mit fremdem WLAN ist das keine theoretische
//! Sorge. Ein Unix-Socket erbt dagegen die Rechte des Dateisystems: Wer die
//! Datei nicht öffnen darf, kommt nicht hinein.
//!
//! Ein Thread je Verbindung, ein Mutex um das Pult. Das ist grob und hier
//! völlig ausreichend: Es fließen Reglerbewegungen, keine Samples, und der
//! Audio-Thread ist an keiner Stelle beteiligt — er sieht nur die lock-freie
//! Kommandoschlange, die das Pult füllt.
//!
//! ```sh
//! nc -U /tmp/musik.sock
//! list deck1.
//! set deck1.play 1
//! ```

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::protokoll::{self, Sitzung};
use crate::pult::Steuerpult;

/// Wie oft nach Änderungen für Abonnenten gesehen wird.
///
/// Fünfzig Millisekunden sind für einen Menschen unmittelbar und für einen
/// Regler feiner, als die Hand ihn bewegt. Häufiger wäre nur mehr Verkehr:
/// Die Abspielposition ändert sich mit jedem Sample, die will niemand
/// vollständig.
const ABO_TAKT: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum ServerFehler {
    Belegt(PathBuf),
    Io(std::io::Error),
}

impl std::fmt::Display for ServerFehler {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ServerFehler::Belegt(p) => write!(
                f,
                "{} wird bereits von einer laufenden Instanz benutzt",
                p.display()
            ),
            ServerFehler::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ServerFehler {}

impl From<std::io::Error> for ServerFehler {
    fn from(e: std::io::Error) -> Self {
        ServerFehler::Io(e)
    }
}

pub struct Server {
    pfad: PathBuf,
}

impl Server {
    /// Startet den Server im Hintergrund.
    ///
    /// Die Verbindung zum Pult läuft über den geteilten Mutex; der Aufrufer
    /// behält seinen eigenen Zugriff und kann weiter bedienen.
    #[cfg(unix)]
    pub fn starten(pfad: &Path, pult: Arc<Mutex<Steuerpult>>) -> Result<Server, ServerFehler> {
        use std::os::unix::net::{UnixListener, UnixStream};

        // Ein liegengebliebener Socket einer abgestürzten Instanz blockiert
        // sonst dauerhaft. Verbinden entscheidet, ob wirklich jemand lauscht:
        // gelingt es, läuft eine zweite Instanz und wir treten zurück.
        if pfad.exists() {
            if UnixStream::connect(pfad).is_ok() {
                return Err(ServerFehler::Belegt(pfad.to_path_buf()));
            }
            std::fs::remove_file(pfad)?;
        }

        if let Some(ordner) = pfad.parent() {
            std::fs::create_dir_all(ordner)?;
        }

        let listener = UnixListener::bind(pfad)?;
        let pfad_kopie = pfad.to_path_buf();

        std::thread::Builder::new()
            .name("control-server".into())
            .spawn(move || {
                for verbindung in listener.incoming() {
                    let Ok(strom) = verbindung else { continue };
                    let pult = Arc::clone(&pult);
                    // Ein Thread je Verbindung. Wer die Anlage steuert, hat
                    // selten mehr als eine Handvoll offen.
                    let _ = std::thread::Builder::new()
                        .name("control-verbindung".into())
                        .spawn(move || bediene(strom, pult));
                }
            })?;

        Ok(Server { pfad: pfad_kopie })
    }

    #[cfg(not(unix))]
    pub fn starten(_pfad: &Path, _pult: Arc<Mutex<Steuerpult>>) -> Result<Server, ServerFehler> {
        Err(ServerFehler::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Unix-Sockets gibt es hier nicht — eine Named Pipe fehlt noch",
        )))
    }

    pub fn pfad(&self) -> &Path {
        &self.pfad
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Aufräumen, damit der nächste Start nicht über die eigene Leiche
        // stolpert.
        let _ = std::fs::remove_file(&self.pfad);
    }
}

#[cfg(unix)]
fn bediene(strom: std::os::unix::net::UnixStream, pult: Arc<Mutex<Steuerpult>>) {
    let Ok(zum_schreiben) = strom.try_clone() else {
        return;
    };

    // Zwei Threads teilen sich die Leitung: dieser antwortet auf Befehle, der
    // andere meldet Änderungen. Beide schreiben, also braucht der Ausgang ein
    // Schloss — sonst schöben sich zwei Zeilen ineinander.
    let aus = Arc::new(Mutex::new(zum_schreiben));
    let sitzung = Arc::new(Mutex::new(Sitzung::neu()));
    let laeuft = Arc::new(AtomicBool::new(true));

    let melder = {
        let aus = Arc::clone(&aus);
        let sitzung = Arc::clone(&sitzung);
        let pult = Arc::clone(&pult);
        let laeuft = Arc::clone(&laeuft);

        std::thread::Builder::new()
            .name("control-abo".into())
            .spawn(move || {
                while laeuft.load(Ordering::Relaxed) {
                    std::thread::sleep(ABO_TAKT);

                    let zeilen = {
                        let (Ok(mut s), Ok(p)) = (sitzung.lock(), pult.lock()) else {
                            continue;
                        };
                        if !s.hat_abos() {
                            continue;
                        }
                        s.aenderungen(&p)
                    };

                    for zeile in zeilen {
                        if !schreibe(&aus, &zeile) {
                            return;
                        }
                    }
                }
            })
            .ok()
    };

    let leser = BufReader::new(strom);
    for zeile in leser.lines() {
        let Ok(zeile) = zeile else { break };

        let antwort = match (pult.lock(), sitzung.lock()) {
            (Ok(mut p), Ok(mut s)) => protokoll::behandle(&mut p, &mut s, &zeile),
            // Ein vergifteter Mutex heißt: irgendwo ist ein Thread mit dem
            // Pult in der Hand gestorben. Weiterbedienen wäre geraten.
            _ => "err Steuerpult ist in einem unklaren Zustand".to_string(),
        };

        if antwort.is_empty() {
            continue;
        }
        if !schreibe(&aus, &antwort) {
            break;
        }
    }

    // Der Melder hängt sonst an einer toten Leitung.
    laeuft.store(false, Ordering::Relaxed);
    if let Some(t) = melder {
        let _ = t.join();
    }
}

#[cfg(unix)]
fn schreibe(aus: &Mutex<std::os::unix::net::UnixStream>, zeile: &str) -> bool {
    let Ok(mut strom) = aus.lock() else {
        return false;
    };
    writeln!(strom, "{zeile}").is_ok() && strom.flush().is_ok()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;
    use std::io::BufRead;
    use std::os::unix::net::UnixStream;

    /// Ein Socketpfad, der zu diesem Testlauf gehört und sonst niemandem.
    fn pfad(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("musik-test-{}-{name}.sock", std::process::id()));
        p
    }

    fn sprich(strom: &UnixStream, zeile: &str) -> String {
        let mut schreiber = strom.try_clone().unwrap();
        writeln!(schreiber, "{zeile}").unwrap();
        schreiber.flush().unwrap();

        let mut leser = BufReader::new(strom.try_clone().unwrap());
        let mut antwort = String::new();
        leser.read_line(&mut antwort).unwrap();
        antwort.trim_end().to_string()
    }

    #[test]
    fn ein_fremder_prozess_kann_die_anlage_bedienen() {
        // Das ist der Punkt der ganzen Übung — und genau das, was Mixxx
        // nicht kann.
        let (pult, _runner) = pult_mit_zwei_decks();
        let pult = Arc::new(Mutex::new(pult));
        let p = pfad("bedienen");
        let server = Server::starten(&p, Arc::clone(&pult)).expect("Server startet nicht");

        let strom = UnixStream::connect(server.pfad()).expect("keine Verbindung");
        assert_eq!(
            sprich(&strom, "set channel1.fader 0.8"),
            "ok channel1.fader 0.800000"
        );
        assert_eq!(
            sprich(&strom, "get channel1.fader"),
            "value channel1.fader 0.800000"
        );

        // Und die Änderung ist wirklich im Pult angekommen, nicht nur in der
        // Antwort.
        let gelesen = pult
            .lock()
            .unwrap()
            .lies(&crate::Schluessel::parse("channel1.fader").unwrap())
            .unwrap();
        assert_eq!(gelesen, crate::Wert::Zahl(0.8));
    }

    #[test]
    fn mehrere_bediener_gleichzeitig() {
        let (pult, _runner) = pult_mit_zwei_decks();
        let p = pfad("mehrere");
        let server = Server::starten(&p, Arc::new(Mutex::new(pult))).unwrap();

        let a = UnixStream::connect(server.pfad()).unwrap();
        let b = UnixStream::connect(server.pfad()).unwrap();

        assert_eq!(sprich(&a, "set deck1.play 1"), "ok deck1.play 1");
        // Der zweite Bediener sieht, was der erste getan hat.
        assert_eq!(sprich(&b, "get deck1.play"), "value deck1.play 1");
    }

    #[test]
    fn abonnenten_bekommen_aenderungen_ungefragt() {
        // Der Unterschied zum Pollen: Der Bediener sagt einmal Bescheid und
        // hört danach zu.
        let (pult, _runner) = pult_mit_zwei_decks();
        let pult = Arc::new(Mutex::new(pult));
        let p = pfad("abo");
        let server = Server::starten(&p, Arc::clone(&pult)).unwrap();

        let strom = UnixStream::connect(server.pfad()).unwrap();
        assert_eq!(sprich(&strom, "sub deck1.play"), "ok sub 1 neu, 1 gesamt");

        let mut leser = BufReader::new(strom.try_clone().unwrap());
        let mut zeile = String::new();

        // Erst der Ist-Zustand …
        leser.read_line(&mut zeile).unwrap();
        assert_eq!(zeile.trim_end(), "value deck1.play 0");

        // … dann die Änderung, ohne dass jemand danach fragt.
        pult.lock()
            .unwrap()
            .schreibe(
                &crate::Schluessel::parse("deck1.play").unwrap(),
                crate::Wert::Schalter(true),
            )
            .unwrap();

        zeile.clear();
        leser.read_line(&mut zeile).unwrap();
        assert_eq!(zeile.trim_end(), "value deck1.play 1");
    }

    #[test]
    fn ein_liegengebliebener_socket_blockiert_den_start_nicht() {
        let p = pfad("leiche");
        std::fs::write(&p, b"").unwrap();

        let (pult, _runner) = pult_mit_zwei_decks();
        let server = Server::starten(&p, Arc::new(Mutex::new(pult)));
        assert!(
            server.is_ok(),
            "abgestürzte Vorgänger dürfen nicht dauerhaft blockieren"
        );
    }

    #[test]
    fn der_socket_verschwindet_beim_beenden() {
        let p = pfad("aufraeumen");
        let (pult, _runner) = pult_mit_zwei_decks();
        {
            let _server = Server::starten(&p, Arc::new(Mutex::new(pult))).unwrap();
            assert!(p.exists());
        }
        assert!(!p.exists(), "der Socket bleibt liegen");
    }
}

/// Zwei Verbindungen gleichzeitig, über den echten Socket.
///
/// Die Zusammenstöße selbst prüft `protokoll::team_tests` — dort ohne Steckdose
/// und deshalb schnell und genau. Hier geht es um das, was nur der volle Weg
/// zeigt: **ob es klemmt.** Der Taktgeber nimmt den Mutex alle 5 ms, jede
/// Verbindung nimmt ihn je Befehl, und der Abo-Thread alle 50 ms. Ob sich diese
/// drei gegenseitig aushungern, sagt kein Modultest.
#[cfg(all(unix, test))]
mod zwei_verbindungen {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;
    use crate::wert::Wert;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    fn k(text: &str) -> crate::Schluessel {
        crate::Schluessel::parse(text).unwrap()
    }

    struct Draht {
        schreiben: UnixStream,
        lesen: BufReader<UnixStream>,
    }

    impl Draht {
        fn neu(pfad: &Path) -> Draht {
            let strom = UnixStream::connect(pfad).expect("verbinden");
            strom
                .set_read_timeout(Some(Duration::from_millis(500)))
                .expect("Zeitlimit");
            Draht {
                lesen: BufReader::new(strom.try_clone().expect("klonen")),
                schreiben: strom,
            }
        }

        fn sagt(&mut self, zeile: &str) {
            writeln!(self.schreiben, "{zeile}").expect("schreiben");
            self.schreiben.flush().expect("leeren");
        }

        /// Liest, bis eine Zeile passt oder die Zeit abläuft.
        fn wartet_auf(&mut self, teil: &str, frist: Duration) -> Option<String> {
            let bis = Instant::now() + frist;
            while Instant::now() < bis {
                let mut zeile = String::new();
                match self.lesen.read_line(&mut zeile) {
                    Ok(0) => return None,
                    Ok(_) => {
                        if zeile.contains(teil) {
                            return Some(zeile.trim_end().to_string());
                        }
                    }
                    Err(_) => continue,
                }
            }
            None
        }
    }

    /// Der ganze Weg: zwei Verbindungen, ein laufender Taktgeber, eine
    /// abgelöste Rampe — und die Meldung kommt beim Verlierer an.
    #[test]
    fn eine_abgeloeste_rampe_erreicht_die_andere_verbindung() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();

        let pfad = std::env::temp_dir().join(format!("musik-team-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&pfad);

        let pult = Arc::new(Mutex::new(pult));
        let server = Server::starten(&pfad, Arc::clone(&pult)).expect("Server");
        let _takt = crate::zeitplan::takt_starten(Arc::clone(&pult));

        // Das Deck muss laufen, sonst steht auch der Plan.
        let laeuft = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let weiter = Arc::clone(&laeuft);
        let audio = std::thread::spawn(move || {
            let mut puffer = vec![0.0f32; 256 * 4];
            while weiter.load(std::sync::atomic::Ordering::Relaxed) {
                runner.render(&mut puffer, 4);
                std::thread::sleep(Duration::from_millis(4));
            }
        });

        let mut a = Draht::neu(server.pfad());
        let mut b = Draht::neu(server.pfad());

        a.sagt("sub master.events");
        assert!(a.wartet_auf("ok sub", Duration::from_secs(2)).is_some());

        a.sagt("ramp channel1.fader 0 64");
        assert!(
            a.wartet_auf("ok plan", Duration::from_secs(2)).is_some(),
            "die Rampe wurde nicht angenommen"
        );

        std::thread::sleep(Duration::from_millis(200));
        b.sagt("set channel1.fader 0.5");

        let gemeldet = a.wartet_auf("abgeloest", Duration::from_secs(3));
        laeuft.store(false, std::sync::atomic::Ordering::Relaxed);
        audio.join().ok();
        let _ = std::fs::remove_file(&pfad);

        assert!(
            gemeldet.is_some(),
            "über den Socket kam keine Ablösung an — der Verlierer bleibt blind"
        );
    }
}
