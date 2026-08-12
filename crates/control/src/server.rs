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
use std::sync::{Arc, Mutex};

use crate::protokoll;
use crate::pult::Steuerpult;

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
    let Ok(schreiber) = strom.try_clone() else {
        return;
    };
    let leser = BufReader::new(strom);
    let mut aus = schreiber;

    for zeile in leser.lines() {
        let Ok(zeile) = zeile else { break };

        let antwort = match pult.lock() {
            Ok(mut p) => protokoll::behandle(&mut p, &zeile),
            // Ein vergifteter Mutex heißt: irgendwo ist ein Thread mit dem
            // Pult in der Hand gestorben. Weiterbedienen wäre geraten.
            Err(_) => "err Steuerpult ist in einem unklaren Zustand".to_string(),
        };

        if antwort.is_empty() {
            continue;
        }
        if writeln!(aus, "{antwort}").is_err() || aus.flush().is_err() {
            break;
        }
    }
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
