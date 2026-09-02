//! Die Steckdose: das Protokoll über einen Unix-Socket — und, wo es den nicht
//! gibt, über die Rückschleife.
//!
//! Ein Socket im Dateisystem und nicht TCP, und das ist eine
//! Sicherheitsentscheidung, keine Bequemlichkeit. Wer hier hineinschreibt,
//! steuert die Anlage. Ein offener TCP-Port täte das für jeden im selben
//! Netz — auf einer Clubbühne mit fremdem WLAN ist das keine theoretische
//! Sorge. Ein Unix-Socket erbt dagegen die Rechte des Dateisystems: Wer die
//! Datei nicht öffnen darf, kommt nicht hinein.
//!
//! # Windows hat keine Unix-Sockets
//!
//! Dort lief die Anlage bisher **ohne jede Steuerung**: Oberfläche, Decks und
//! Mixer ja, Fernsteuerung nein — und damit auch kein MCP und keine Agenten,
//! also genau der Teil, um dessentwillen das Projekt gebaut wird.
//!
//! Deshalb gibt es einen zweiten Weg, und er ist **enger gebaut als der
//! erste**, weil TCP von sich aus weiter offen steht:
//!
//! 1. **Nur die Rückschleife.** Gebunden wird ausschließlich an `127.0.0.1`
//!    oder `::1`. Eine andere Adresse wird abgelehnt, nicht stillschweigend
//!    übernommen — ein Tippfehler soll die Anlage nicht ins WLAN stellen.
//! 2. **Ein Schlüssel.** Die erste Zeile einer Verbindung muss `auth
//!    <schlüssel>` sein, sonst wird nichts ausgeführt. Der Schlüssel entsteht
//!    beim Start neu und steht nur im Fenster der Anwendung.
//!
//! Der zweite Punkt ist nicht Zierrat. Die Rückschleife ist nicht privat: Jedes
//! Programm auf demselben Rechner erreicht sie, **eine Webseite im Browser
//! eingeschlossen**. Ein `fetch` an `http://127.0.0.1:<port>` schickt Zeilen,
//! die dieses Protokoll liest; ohne Schlüssel könnte eine beliebige Seite im
//! Hintergrund den Crossfader ziehen. Lesen kann sie ihn nicht — die Antwort
//! bleibt ihr durch die Regeln des Browsers verborgen, und ohne die erste Zeile
//! kommt sie nicht weiter.
//!
//! Wo es einen Unix-Socket gibt, bleibt er die erste Wahl: Dort erledigt das
//! Dateisystem, wofür hier ein Schlüssel nötig ist.
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
//!
//! ```text
//! # über die Rückschleife, Schlüssel zuerst
//! nc 127.0.0.1 7657
//! auth 7f3a1c…
//! ok angemeldet
//! set deck1.play 1
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
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
    /// Es sollte an etwas gebunden werden, das nicht die Rückschleife ist.
    NichtLokal(SocketAddr),
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
            ServerFehler::NichtLokal(a) => write!(
                f,
                "{a} ist nicht die Rückschleife — die Anlage wird nicht ins Netz \
                 gestellt. Erlaubt sind 127.0.0.1 und ::1."
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
    /// Der Socketpfad — nur beim Weg über das Dateisystem, und nur der wird
    /// beim Aufräumen gelöscht.
    pfad: Option<PathBuf>,
    /// Wie man hinkommt, für Menschen.
    adresse: String,
    /// Der Schlüssel, den eine Verbindung zuerst nennen muss. `None` beim
    /// Unix-Socket: Dort hat das Dateisystem die Frage schon beantwortet.
    schluessel: Option<String>,
}

/// Eine Leitung, über die das Protokoll läuft.
///
/// Zwei Threads teilen sie sich — einer antwortet, einer meldet —, deshalb muss
/// sie sich verdoppeln lassen. Mehr wird nicht gebraucht, und genau deshalb
/// steht hier ein eigener kleiner Vertrag statt zweier fast gleicher Kopien
/// der Bedienung.
trait Leitung: Read + Write + Send + Sized + 'static {
    fn klonen(&self) -> std::io::Result<Self>;
}

#[cfg(unix)]
impl Leitung for std::os::unix::net::UnixStream {
    fn klonen(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
}

impl Leitung for TcpStream {
    fn klonen(&self) -> std::io::Result<Self> {
        self.try_clone()
    }
}

/// Ein Schlüssel aus dem Zufall des Betriebssystems.
///
/// Nicht aus der Uhrzeit: Wer weiß, wann die Anwendung gestartet wurde, hätte
/// den Schlüssel damit fast schon. Sechzehn Byte als Hex sind kurz genug zum
/// Abtippen und lang genug, dass Raten ausscheidet.
fn schluessel_erzeugen() -> String {
    let mut roh = [0u8; 16];
    getrandom::fill(&mut roh).expect("kein Zufall vom Betriebssystem");
    roh.iter().map(|b| format!("{b:02x}")).collect()
}

/// Ob eine Adresse nur von diesem Rechner aus erreichbar ist.
fn ist_rueckschleife(adresse: &SocketAddr) -> bool {
    adresse.ip().is_loopback()
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
                        .spawn(move || bediene(strom, pult, None));
                }
            })?;

        Ok(Server {
            adresse: pfad_kopie.display().to_string(),
            pfad: Some(pfad_kopie),
            schluessel: None,
        })
    }

    #[cfg(not(unix))]
    pub fn starten(_pfad: &Path, _pult: Arc<Mutex<Steuerpult>>) -> Result<Server, ServerFehler> {
        Err(ServerFehler::Io(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Unix-Sockets gibt es hier nicht — nimm starten_tcp",
        )))
    }

    /// Startet den Server auf der Rückschleife, mit Schlüssel.
    ///
    /// Für Windows gedacht, wo es keine Unix-Sockets gibt; anderswo geht es
    /// auch, ist aber der schwächere Weg — siehe den Kopf dieser Datei.
    pub fn starten_tcp(
        adresse: SocketAddr,
        pult: Arc<Mutex<Steuerpult>>,
    ) -> Result<Server, ServerFehler> {
        // **Vor** dem Binden prüfen. Danach wäre der Port schon offen, und
        // zwischen Öffnen und Schließen liegt genau das Fenster, das man nicht
        // haben will.
        if !ist_rueckschleife(&adresse) {
            return Err(ServerFehler::NichtLokal(adresse));
        }

        let listener = TcpListener::bind(adresse)?;
        let echte = listener.local_addr()?;
        let schluessel = schluessel_erzeugen();
        let fuer_threads = Arc::new(schluessel.clone());

        std::thread::Builder::new()
            .name("control-server".into())
            .spawn(move || {
                for verbindung in listener.incoming() {
                    let Ok(strom) = verbindung else { continue };
                    // Nagle aus: Es fließen kurze Zeilen, und die sollen sofort
                    // ankommen statt auf Gesellschaft zu warten.
                    let _ = strom.set_nodelay(true);
                    let pult = Arc::clone(&pult);
                    let schluessel = Arc::clone(&fuer_threads);
                    let _ = std::thread::Builder::new()
                        .name("control-verbindung".into())
                        .spawn(move || bediene(strom, pult, Some(schluessel)));
                }
            })?;

        Ok(Server {
            pfad: None,
            adresse: echte.to_string(),
            schluessel: Some(schluessel),
        })
    }

    /// Der Socketpfad — `None`, wenn über die Rückschleife gelauscht wird.
    pub fn pfad(&self) -> Option<&Path> {
        self.pfad.as_deref()
    }

    /// Wie man hinkommt, als Text für Menschen.
    pub fn adresse(&self) -> &str {
        &self.adresse
    }

    /// Der Schlüssel, falls einer nötig ist.
    pub fn schluessel(&self) -> Option<&str> {
        self.schluessel.as_deref()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Aufräumen, damit der nächste Start nicht über die eigene Leiche
        // stolpert. Ein Port hinterlässt nichts, was aufzuräumen wäre.
        if let Some(pfad) = &self.pfad {
            let _ = std::fs::remove_file(pfad);
        }
    }
}

/// Bedient eine Verbindung, bis sie abbricht.
///
/// `schluessel` ist `Some`, wenn erst angemeldet werden muss — beim Weg über
/// die Rückschleife. Solange das nicht geschehen ist, wird **kein** Befehl
/// ausgeführt, auch kein lesender: Wer nicht hereindarf, soll auch nicht
/// erfahren, was gerade läuft.
fn bediene<L: Leitung>(strom: L, pult: Arc<Mutex<Steuerpult>>, schluessel: Option<Arc<String>>) {
    let Ok(zum_schreiben) = strom.klonen() else {
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

    // Beim Unix-Socket hat das Dateisystem schon entschieden; dort gilt die
    // Verbindung von Anfang an als angemeldet.
    let mut angemeldet = schluessel.is_none();

    let leser = BufReader::new(strom);
    for zeile in leser.lines() {
        let Ok(zeile) = zeile else { break };

        if !angemeldet {
            let antwort = match anmeldung_pruefen(&zeile, schluessel.as_deref()) {
                true => {
                    angemeldet = true;
                    "ok angemeldet".to_string()
                }
                // Kein Hinweis darauf, was falsch war, und keine Auskunft über
                // die Anlage. Wer raten will, soll nichts dazulernen.
                false => "err nicht angemeldet — erste Zeile: auth <schlüssel>".to_string(),
            };
            if !schreibe(&aus, &antwort) {
                break;
            }
            continue;
        }

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

/// Ob diese Zeile die richtige Anmeldung ist.
///
/// Verglichen wird über die volle Länge, nicht mit `==` auf dem ersten
/// Unterschied — ein Vergleich, der früh abbricht, verrät über die Zeit, wie
/// viele Zeichen stimmten. Hier ist das kaum auszunutzen, aber der richtige
/// Vergleich kostet nichts.
fn anmeldung_pruefen(zeile: &str, schluessel: Option<&String>) -> bool {
    let Some(erwartet) = schluessel else {
        return true;
    };
    let Some(gegeben) = zeile.trim().strip_prefix("auth ") else {
        return false;
    };
    let gegeben = gegeben.trim().as_bytes();
    let erwartet = erwartet.as_bytes();
    if gegeben.len() != erwartet.len() {
        return false;
    }
    let mut unterschied = 0u8;
    for (a, b) in gegeben.iter().zip(erwartet) {
        unterschied |= a ^ b;
    }
    unterschied == 0
}

fn schreibe<L: Leitung>(aus: &Mutex<L>, zeile: &str) -> bool {
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

        let strom = UnixStream::connect(server.pfad().unwrap()).expect("keine Verbindung");
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

        let a = UnixStream::connect(server.pfad().unwrap()).unwrap();
        let b = UnixStream::connect(server.pfad().unwrap()).unwrap();

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

        let strom = UnixStream::connect(server.pfad().unwrap()).unwrap();
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

        let mut a = Draht::neu(server.pfad().unwrap());
        let mut b = Draht::neu(server.pfad().unwrap());

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

/// Der Weg über die Rückschleife — geprüft ohne Unix-Sockets, damit er auch
/// dort gilt, wo es keine gibt.
#[cfg(test)]
mod tcp {
    use super::*;
    use crate::testing::pult_mit_zwei_decks;
    use std::io::BufRead;
    use std::net::{IpAddr, Ipv4Addr};

    fn pult() -> Arc<Mutex<Steuerpult>> {
        let (pult, runner) = pult_mit_zwei_decks();
        // Der Runner muss am Leben bleiben, sonst bricht die Kommandoschlange
        // und jeder Befehl liefe ins Nichts — die Tests würden grün, obwohl
        // nichts ankommt.
        std::mem::forget(runner);
        Arc::new(Mutex::new(pult))
    }

    /// Ein Port, den nur dieser Lauf benutzt: 0 heißt „such dir einen".
    fn irgendwo() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    }

    struct Draht {
        schreiben: TcpStream,
        lesen: BufReader<TcpStream>,
    }

    impl Draht {
        fn neu(adresse: &str) -> Draht {
            let strom = TcpStream::connect(adresse).expect("verbinden");
            let lesen = BufReader::new(strom.try_clone().expect("klonen"));
            Draht {
                schreiben: strom,
                lesen,
            }
        }

        fn sag(&mut self, zeile: &str) -> String {
            writeln!(self.schreiben, "{zeile}").expect("schreiben");
            self.schreiben.flush().expect("leeren");
            let mut antwort = String::new();
            self.lesen.read_line(&mut antwort).expect("lesen");
            antwort.trim_end().to_string()
        }
    }

    /// **Die Anlage wird nicht ins Netz gestellt.**
    ///
    /// Ein Tippfehler in der Adresse — `0.0.0.0` statt `127.0.0.1` — würde
    /// jeden im selben WLAN ans Pult lassen. Das muss abgelehnt werden, und
    /// zwar *bevor* gebunden wird: Zwischen Öffnen und Schließen läge sonst
    /// genau das Fenster, das man nicht haben will.
    #[test]
    fn eine_adresse_ausserhalb_der_rueckschleife_wird_abgelehnt() {
        let offen = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        let fehler = Server::starten_tcp(offen, pult())
            .err()
            .expect("darf nicht binden");
        assert!(
            matches!(fehler, ServerFehler::NichtLokal(_)),
            "falscher Fehler: {fehler}"
        );
        assert!(
            TcpStream::connect(offen).is_err(),
            "es wurde trotzdem gebunden"
        );
    }

    /// **Ohne Schlüssel geschieht nichts** — auch nichts Lesendes.
    #[test]
    fn ohne_anmeldung_wird_kein_befehl_ausgefuehrt() {
        let server = Server::starten_tcp(irgendwo(), pult()).expect("starten");
        let mut draht = Draht::neu(server.adresse());

        let antwort = draht.sag("set deck1.play 1");
        assert!(antwort.starts_with("err nicht angemeldet"), "{antwort}");

        // Auch fragen ist Auskunft. Wer nicht hereindarf, soll nicht erfahren,
        // was gerade läuft.
        let antwort = draht.sag("get deck1.play");
        assert!(antwort.starts_with("err nicht angemeldet"), "{antwort}");
    }

    #[test]
    fn ein_falscher_schluessel_hilft_nicht() {
        let server = Server::starten_tcp(irgendwo(), pult()).expect("starten");
        let mut draht = Draht::neu(server.adresse());
        let antwort = draht.sag("auth 00000000000000000000000000000000");
        assert!(antwort.starts_with("err nicht angemeldet"), "{antwort}");
    }

    #[test]
    fn mit_dem_richtigen_schluessel_laesst_sich_bedienen() {
        let server = Server::starten_tcp(irgendwo(), pult()).expect("starten");
        let schluessel = server.schluessel().expect("Schlüssel").to_string();
        let mut draht = Draht::neu(server.adresse());

        assert_eq!(draht.sag(&format!("auth {schluessel}")), "ok angemeldet");
        assert_eq!(draht.sag("set deck1.play 1"), "ok deck1.play 1");
        assert_eq!(draht.sag("get deck1.play"), "value deck1.play 1");
    }

    /// Jeder Start bekommt einen eigenen Schlüssel. Ein fester wäre nach dem
    /// ersten Mal keiner mehr.
    #[test]
    fn jeder_start_hat_einen_eigenen_schluessel() {
        let a = Server::starten_tcp(irgendwo(), pult()).expect("starten");
        let b = Server::starten_tcp(irgendwo(), pult()).expect("starten");
        assert_ne!(a.schluessel(), b.schluessel());
        assert_eq!(a.schluessel().map(str::len), Some(32));
    }

    #[test]
    fn der_vergleich_haengt_nicht_an_der_laenge_allein() {
        let s = "abcdef".to_string();
        assert!(anmeldung_pruefen("auth abcdef", Some(&s)));
        assert!(!anmeldung_pruefen("auth abcde", Some(&s)));
        assert!(!anmeldung_pruefen("auth abcdefg", Some(&s)));
        assert!(!anmeldung_pruefen("abcdef", Some(&s)));
        // Ohne Schlüssel ist alles angemeldet — der Unix-Socket.
        assert!(anmeldung_pruefen("was auch immer", None));
    }
}
