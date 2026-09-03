//! Was gemeint war — die Mitschrift neben dem Mitschnitt.
//!
//! Der Mitschnitt sagt, was herauskam. Er sagt nicht, was beabsichtigt war, und
//! deshalb kann der Kritiker zwei Dinge nicht, die er können müsste:
//!
//! **Den Beginn einer langen Blende findet er im Klang nicht.** Am Anfang ist
//! der eingehende Track per Konstruktion unhörbar — das ist, was eine Blende
//! ausmacht. Bei sechzehn Sekunden liegt seine Schätzung um vier daneben, und
//! das ist keine Ungenauigkeit, die sich wegrechnen ließe: Die Information
//! steht in der Summe nicht drin.
//!
//! **Die Phrasenlage kann er gar nicht messen.** Der Anker eines nachträglich
//! geschätzten Rasters ist der erste starke Schlag im Analysefenster, nicht der
//! Anfang einer Phrase. Jede Angabe „so viele Beats neben der Eins" wäre gegen
//! einen willkürlichen Nullpunkt gemessen.
//!
//! Beides weiß die Anlage im Moment des Geschehens genau. Sie kennt den Frame,
//! an dem der Befehl ankam, und sie kennt den Beat, auf dem jedes Deck dabei
//! stand. Wer das mitschreibt, muss es hinterher nicht erraten.
//!
//! **Das ist ausdrücklich keine zweite Wahrheit.** Die Mitschrift sagt, wann
//! der Griff an den Fader geschah; ob dabei etwas Gutes herauskam, sagt
//! weiterhin nur der Mitschnitt. Erst nebeneinander ergeben sie ein Urteil —
//! und der Abstand zwischen beiden ist selbst eine Zahl, die es wert ist,
//! aufgeschrieben zu werden: Er misst, wie weit der Kritiker danebenliegt, wenn
//! er raten muss.
//!
//! # Form
//!
//! Zeilenweise Text, damit man sie ohne Werkzeug lesen kann. Ein Kopf aus
//! Kommentaren, dann je Ereignis eine Zeile:
//!
//! ```text
//! # musik-mitschrift 1
//! # mitschnitt /pfad/set.wav
//! # rate 48000
//! 120960 2.520 deck1=64.000/16 deck2=-0.998/16~ > ramp master.crossfader 1 32
//! 133440 2.780 deck1=70.000/16 deck2=5.000/16 < ok plan 3 ramp master.crossfader nach 1
//! ```
//!
//! Ein Deckfeld ist `deck<N>=<beat>/<phrase>`; ein angehängtes `~` heißt, dass
//! das Deck **stand**. Das ist kein Beiwerk: Ein wartendes Deck steht irgendwo
//! zwischen zwei Phrasengrenzen, und ohne die Markierung liest sich das später
//! wie „es kam neben der Eins herein" — obwohl es überhaupt nicht hereinkam.
//!
//! `>` ist hereingekommen, `<` ist hinausgegangen — Absicht und Antwort. Die
//! Sekunden sind eine Bequemlichkeit für den lesenden Menschen; **maßgeblich
//! ist der Frame**, und der Leser rechnet sie aus der Rate im Kopf neu aus.
//! Zwei Zahlen, die dasselbe sagen sollen, driften sonst irgendwann
//! auseinander, und dann glaubt man der falschen.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Fassung der Form. Steht im Kopf, damit ein Leser eine spätere erkennt.
pub const FORMAT: u32 = 1;

/// Die Endung, die neben dem Mitschnitt liegt.
pub const ENDUNG: &str = "mitschrift";

/// Woher eine Zeile stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Richtung {
    /// Ein Befehl, der hereinkam — die Absicht.
    Befehl,
    /// Was die Anlage darauf meldete — das Ergebnis.
    Meldung,
}

impl Richtung {
    pub fn zeichen(&self) -> char {
        match self {
            Richtung::Befehl => '>',
            Richtung::Meldung => '<',
        }
    }

    pub fn parse(text: &str) -> Option<Richtung> {
        match text {
            ">" => Some(Richtung::Befehl),
            "<" => Some(Richtung::Meldung),
            _ => None,
        }
    }
}

/// Das Zeichen für ein Deck, das nicht lief.
///
/// Steht am Ende des Feldes: `deck2=-0.998/16~`.
pub const STEHT: char = '~';

/// Wo ein Deck stand, als etwas geschah.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stand {
    /// Nullbasiert; in der Zeile steht `deck1` für 0.
    pub deck: usize,
    /// Beat auf dem Beatgrid des Decks.
    pub beat: f64,
    /// Wie viele Beats die Anlage als eine Phrase zählt.
    pub phrase_beats: f64,
    /// Ob das Deck lief.
    ///
    /// **Ohne das lädt die Phrasenlage eines stehenden Decks zu einem falschen
    /// Schluss ein.** Ein Deck, das am Anfang seines Tracks wartet, steht
    /// irgendwo zwischen zwei Phrasengrenzen — das heißt nicht, dass es neben
    /// der Eins eingesetzt hätte. Es hat überhaupt nicht eingesetzt.
    pub laeuft: bool,
}

impl Stand {
    /// Wie weit hinter der letzten Phrasengrenze, in Beats.
    ///
    /// `None`, wenn das Deck stand: Dann ist die Zahl zwar ausrechenbar, aber
    /// sie beantwortet keine Frage, die jemand gestellt hätte.
    ///
    /// **Gemessen gegen die Rechnung der Anlage, nicht gegen die Musik.** Ob
    /// Beat 0 des Rasters wirklich ein Downbeat ist, weiß erst die
    /// Strukturanalyse (S2). Bis dahin beantwortet diese Zahl die kleinere,
    /// aber immerhin beantwortbare Frage: Hat die Anlage getan, was sie selbst
    /// vorhatte?
    pub fn phrasenlage(&self) -> Option<f64> {
        if !self.laeuft || self.phrase_beats <= 0.0 {
            return None;
        }
        Some(self.beat.rem_euclid(self.phrase_beats))
    }

    fn feld(&self) -> String {
        let mut feld = format!(
            "deck{}={:.3}/{}",
            self.deck + 1,
            self.beat,
            self.phrase_beats
        );
        if !self.laeuft {
            feld.push(STEHT);
        }
        feld
    }

    fn parse(feld: &str) -> Option<Stand> {
        let rest = feld.strip_prefix("deck")?;
        let (nummer, rest) = rest.split_once('=')?;
        let (beat, phrase) = rest.split_once('/')?;
        let nummer: usize = nummer.parse().ok()?;
        let (phrase, laeuft) = match phrase.strip_suffix(STEHT) {
            Some(ohne) => (ohne, false),
            None => (phrase, true),
        };
        Some(Stand {
            deck: nummer.checked_sub(1)?,
            beat: beat.parse().ok()?,
            phrase_beats: phrase.parse().ok()?,
            laeuft,
        })
    }
}

/// Ein Ereignis in der Mitschrift.
#[derive(Debug, Clone, PartialEq)]
pub struct Ereignis {
    /// Position im Mitschnitt, in Frames. Die maßgebliche Zeitangabe.
    pub frame: u64,
    /// Wo die Decks standen. Nur die mit Beatgrid stehen drin — ein Deck ohne
    /// Raster hat keinen Beat, auf den sich etwas beziehen ließe.
    pub staende: Vec<Stand>,
    pub richtung: Richtung,
    pub text: String,
}

impl Ereignis {
    pub fn sekunden(&self, rate: u32) -> f64 {
        if rate == 0 {
            return 0.0;
        }
        self.frame as f64 / rate as f64
    }

    pub fn stand(&self, deck: usize) -> Option<&Stand> {
        self.staende.iter().find(|s| s.deck == deck)
    }

    /// Die Zeile, wie sie in der Datei steht.
    pub fn zeile(&self, rate: u32) -> String {
        let mut zeile = format!("{} {:.3}", self.frame, self.sekunden(rate));
        for s in &self.staende {
            zeile.push(' ');
            zeile.push_str(&s.feld());
        }
        zeile.push(' ');
        zeile.push(self.richtung.zeichen());
        zeile.push(' ');
        zeile.push_str(&self.text);
        zeile
    }

    /// Liest eine Zeile zurück.
    ///
    /// `None` für alles, was nicht passt — eine abgeschnittene letzte Zeile
    /// nach einem Absturz darf nicht die ganze Datei unlesbar machen.
    pub fn parse(zeile: &str) -> Option<Ereignis> {
        let zeile = zeile.trim_end_matches(['\r', '\n']);
        let mut worte = zeile.split_whitespace();
        let frame: u64 = worte.next()?.parse().ok()?;
        // Die Sekunden werden gelesen und verworfen: Maßgeblich ist der Frame.
        worte.next()?.parse::<f64>().ok()?;

        let mut staende = Vec::new();
        let richtung = loop {
            let wort = worte.next()?;
            if let Some(r) = Richtung::parse(wort) {
                break r;
            }
            staende.push(Stand::parse(wort)?);
        };

        // Der Text ist der Rest der Zeile, mit den ursprünglichen Abständen.
        // Über die Wortgrenzen ließe er sich nicht wiederherstellen.
        let marke = format!(" {} ", richtung.zeichen());
        let text = zeile.split_once(&marke)?.1.to_string();

        Some(Ereignis {
            frame,
            staende,
            richtung,
            text,
        })
    }
}

/// Der Kopf der Datei.
#[derive(Debug, Clone, PartialEq)]
pub struct Kopf {
    pub format: u32,
    /// Zu welchem Mitschnitt die Mitschrift gehört.
    pub mitschnitt: String,
    pub rate: u32,
}

/// Eine gelesene Mitschrift.
#[derive(Debug, Clone)]
pub struct Protokoll {
    pub kopf: Kopf,
    pub ereignisse: Vec<Ereignis>,
    /// Wie viele Zeilen sich nicht lesen ließen.
    ///
    /// Steht hier, statt still übersprungen zu werden: Eine Mitschrift mit
    /// Lücken, die aussieht wie eine ohne, wäre das schlechteste von allem —
    /// dieselbe Überlegung wie bei den verworfenen Frames im Mitschnitt.
    pub unlesbar: usize,
}

impl Protokoll {
    /// Alle Ereignisse, deren Text mit einem der Präfixe beginnt.
    pub fn mit_praefix<'a>(
        &'a self,
        praefixe: &'a [&'a str],
    ) -> impl Iterator<Item = &'a Ereignis> {
        self.ereignisse
            .iter()
            .filter(move |e| praefixe.iter().any(|p| e.text.starts_with(p)))
    }
}

/// Liest eine Mitschrift aus Text.
pub fn aus_text(text: &str) -> Result<Protokoll, String> {
    let mut format = None;
    let mut mitschnitt = String::new();
    let mut rate = None;
    let mut ereignisse = Vec::new();
    let mut unlesbar = 0usize;

    for zeile in text.lines() {
        let getrimmt = zeile.trim();
        if getrimmt.is_empty() {
            continue;
        }
        if let Some(kopf) = getrimmt.strip_prefix('#') {
            let kopf = kopf.trim();
            if let Some(wert) = kopf.strip_prefix("musik-mitschrift ") {
                format = wert.trim().parse().ok();
            } else if let Some(wert) = kopf.strip_prefix("mitschnitt ") {
                mitschnitt = wert.trim().to_string();
            } else if let Some(wert) = kopf.strip_prefix("rate ") {
                rate = wert.trim().parse().ok();
            }
            continue;
        }
        match Ereignis::parse(zeile) {
            Some(e) => ereignisse.push(e),
            None => unlesbar += 1,
        }
    }

    let Some(format) = format else {
        return Err("kein Kopf — das ist keine Mitschrift".into());
    };
    if format > FORMAT {
        return Err(format!(
            "Fassung {format} ist neuer als die bekannte {FORMAT}"
        ));
    }
    let Some(rate) = rate.filter(|r| *r > 0) else {
        return Err("keine Abtastrate im Kopf — Frames wären ohne sie zeitlos".into());
    };

    Ok(Protokoll {
        kopf: Kopf {
            format,
            mitschnitt,
            rate,
        },
        ereignisse,
        unlesbar,
    })
}

/// Liest eine Mitschrift von der Platte.
pub fn lesen(pfad: &Path) -> Result<Protokoll, String> {
    let text = std::fs::read_to_string(pfad).map_err(|e| format!("{}: {e}", pfad.display()))?;
    aus_text(&text)
}

/// Der Schreiber, der neben dem Mitschnitt mitläuft.
///
/// **Er schreibt sofort durch.** Ein gepufferter Schreiber verlöre bei einem
/// Absturz genau die Zeilen, die erklären würden, was zuletzt geschah. Das ist
/// vertretbar, weil hier nicht der Audio-Callback schreibt, sondern der
/// Steuer-Thread, und weil ein Set ein paar hundert Ereignisse hat und nicht
/// ein paar hunderttausend.
pub struct Mitschrift {
    datei: BufWriter<File>,
    pfad: PathBuf,
    rate: u32,
    fehler: u64,
}

impl Mitschrift {
    /// Legt die Mitschrift neben einen Mitschnitt.
    pub fn starten(pfad: &Path, mitschnitt: &Path, rate: u32) -> Result<Mitschrift, String> {
        let datei = File::create(pfad).map_err(|e| format!("{}: {e}", pfad.display()))?;
        let mut m = Mitschrift {
            datei: BufWriter::new(datei),
            pfad: pfad.to_path_buf(),
            rate,
            fehler: 0,
        };
        let kopf = format!(
            "# musik-mitschrift {FORMAT}\n# mitschnitt {}\n# rate {rate}\n",
            mitschnitt.display()
        );
        m.roh(&kopf);
        Ok(m)
    }

    /// Wo die Mitschrift liegt.
    pub fn pfad(&self) -> &Path {
        &self.pfad
    }

    /// Wie viele Zeilen sich nicht schreiben ließen.
    pub fn fehler(&self) -> u64 {
        self.fehler
    }

    /// Hält ein Ereignis fest.
    ///
    /// Mehrzeilige Antworten werden auf ihre erste Zeile gekürzt: Eine Suche
    /// mit vierzig Treffern würde die Mitschrift sonst zuschütten, und was sie
    /// festhalten soll, ist die Bewegung, nicht der Katalog.
    pub fn halten(&mut self, frame: u64, staende: &[Stand], richtung: Richtung, text: &str) {
        let mut zeilen = text.lines().filter(|z| !z.trim().is_empty());
        let Some(erste) = zeilen.next() else {
            return;
        };
        let weitere = zeilen.count();
        let text = if weitere > 0 {
            format!("{erste} … (+{weitere} Zeilen)")
        } else {
            erste.to_string()
        };

        let ereignis = Ereignis {
            frame,
            staende: staende.to_vec(),
            richtung,
            text,
        };
        let zeile = ereignis.zeile(self.rate);
        self.roh(&zeile);
        self.roh("\n");
    }

    fn roh(&mut self, text: &str) {
        // Ein Schreibfehler mitten im Set darf weder den Mix anhalten noch
        // stillschweigend verschwinden. Also zählen und beim Stoppen melden —
        // dieselbe Regel wie bei den verworfenen Frames.
        if self.datei.write_all(text.as_bytes()).is_err() || self.datei.flush().is_err() {
            self.fehler += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stand(deck: usize, beat: f64) -> Stand {
        Stand {
            deck,
            beat,
            phrase_beats: 16.0,
            laeuft: true,
        }
    }

    fn stehend(deck: usize, beat: f64) -> Stand {
        Stand {
            laeuft: false,
            ..stand(deck, beat)
        }
    }

    #[test]
    fn eine_zeile_ueberlebt_hin_und_zurueck() {
        let e = Ereignis {
            frame: 120_960,
            staende: vec![stand(0, 64.0), stand(1, 3.5)],
            richtung: Richtung::Befehl,
            text: "ramp master.crossfader 1 32".into(),
        };
        let zeile = e.zeile(48_000);
        assert_eq!(
            zeile,
            "120960 2.520 deck1=64.000/16 deck2=3.500/16 > ramp master.crossfader 1 32"
        );
        assert_eq!(Ereignis::parse(&zeile), Some(e));
    }

    /// Der Text darf alles enthalten, auch die Zeichen, die sonst die Richtung
    /// markieren. Sonst zerfiele jede Meldung mit einem Vergleich darin.
    #[test]
    fn ein_text_mit_spitzen_klammern_bleibt_heil() {
        let e = Ereignis {
            frame: 0,
            staende: Vec::new(),
            richtung: Richtung::Befehl,
            text: "when deck1.beats_left < 32 do master.queue_next".into(),
        };
        let zurueck = Ereignis::parse(&e.zeile(48_000)).expect("nicht lesbar");
        assert_eq!(
            zurueck.text,
            "when deck1.beats_left < 32 do master.queue_next"
        );
        assert_eq!(zurueck.richtung, Richtung::Befehl);
    }

    /// Der Text ist der Rest der Zeile, nicht eine Liste von Wörtern.
    ///
    /// Das Protokoll lässt zwischen zwei Wörtern mehrere Leerzeichen stehen,
    /// damit ein Wert später einmal welche enthalten darf. Wer die Zeile über
    /// Wortgrenzen wieder zusammensetzt, ändert genau diese Befehle still ab —
    /// und die Mitschrift behauptete dann etwas anderes, als gesagt wurde.
    #[test]
    fn mehrere_leerzeichen_im_text_ueberleben() {
        let e = Ereignis {
            frame: 0,
            staende: Vec::new(),
            richtung: Richtung::Befehl,
            text: "set deck1.title Zwei  Leerzeichen".into(),
        };
        let zurueck = Ereignis::parse(&e.zeile(48_000)).expect("nicht lesbar");
        assert_eq!(zurueck.text, "set deck1.title Zwei  Leerzeichen");
    }

    /// Ein Deck ohne Beatgrid steht gar nicht in der Zeile — es hat keinen
    /// Beat, auf den sich etwas beziehen ließe.
    #[test]
    fn ohne_stand_geht_es_auch() {
        let e = Ereignis {
            frame: 4_800,
            staende: Vec::new(),
            richtung: Richtung::Meldung,
            text: "ok".into(),
        };
        let zeile = e.zeile(48_000);
        assert_eq!(zeile, "4800 0.100 < ok");
        assert_eq!(Ereignis::parse(&zeile), Some(e));
    }

    /// Maßgeblich ist der Frame. Stünde in der Datei eine abweichende
    /// Sekundenangabe, dürfte sie nichts verschieben.
    #[test]
    fn die_sekunden_in_der_zeile_sind_nur_bequemlichkeit() {
        let e = Ereignis::parse("96000 999.999 > set channel1.fader 0.5").expect("nicht lesbar");
        assert_eq!(e.frame, 96_000);
        assert!((e.sekunden(48_000) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn die_phrasenlage_zaehlt_ab_der_letzten_grenze() {
        assert_eq!(stand(0, 64.0).phrasenlage(), Some(0.0));
        assert!((stand(0, 68.25).phrasenlage().unwrap() - 4.25).abs() < 1e-9);
        // Vor dem Nullpunkt darf sie nicht negativ werden.
        assert!((stand(0, -1.0).phrasenlage().unwrap() - 15.0).abs() < 1e-9);
    }

    /// Ein stehendes Deck hat keine Phrasenlage.
    ///
    /// Ausrechenbar wäre sie — es steht ja irgendwo zwischen zwei Grenzen.
    /// Aber sie beantwortete die Frage „kam es neben der Eins?" mit einer Zahl
    /// über ein Deck, das gar nicht eingesetzt hat. Genau so entsteht ein
    /// Bericht, der etwas behauptet, das nie geschehen ist.
    #[test]
    fn ein_stehendes_deck_hat_keine_phrasenlage() {
        assert_eq!(stehend(1, -0.998).phrasenlage(), None);
        assert_eq!(stehend(1, 8.0).phrasenlage(), None);
    }

    #[test]
    fn ob_ein_deck_lief_ueberlebt_die_zeile() {
        let e = Ereignis {
            frame: 0,
            staende: vec![stand(0, 48.0), stehend(1, -0.998)],
            richtung: Richtung::Befehl,
            text: "ramp master.crossfader 1 32".into(),
        };
        let zeile = e.zeile(48_000);
        assert!(zeile.contains("deck2=-0.998/16~"), "{zeile}");
        assert!(zeile.contains("deck1=48.000/16 "), "{zeile}");
        assert_eq!(Ereignis::parse(&zeile), Some(e));
    }

    fn beispiel() -> String {
        [
            "# musik-mitschrift 1",
            "# mitschnitt /pfad/set.wav",
            "# rate 48000",
            "0 0.000 deck1=0.000/16 > do deck1.play",
            "120960 2.520 deck1=64.000/16 > ramp master.crossfader 1 32",
            "121000 2.521 deck1=64.021/16 < ok plan 3 ramp master.crossfader nach 1",
        ]
        .join("\n")
    }

    #[test]
    fn eine_ganze_mitschrift_laesst_sich_lesen() {
        let p = aus_text(&beispiel()).expect("nicht lesbar");
        assert_eq!(p.kopf.rate, 48_000);
        assert_eq!(p.kopf.mitschnitt, "/pfad/set.wav");
        assert_eq!(p.ereignisse.len(), 3);
        assert_eq!(p.unlesbar, 0);
        assert_eq!(p.ereignisse[1].frame, 120_960);
        assert_eq!(p.ereignisse[1].stand(0).map(|s| s.beat), Some(64.0));
    }

    /// Ein Absturz schneidet die letzte Zeile ab. Der Rest muss trotzdem
    /// lesbar bleiben — und die Lücke muss dastehen.
    #[test]
    fn eine_abgeschnittene_zeile_macht_nicht_die_datei_unlesbar() {
        let text = format!("{}\n1210", beispiel());
        let p = aus_text(&text).expect("nicht lesbar");
        assert_eq!(p.ereignisse.len(), 3);
        assert_eq!(p.unlesbar, 1, "die Lücke wurde verschwiegen");
    }

    #[test]
    fn ohne_kopf_ist_es_keine_mitschrift() {
        assert!(aus_text("0 0.000 > do deck1.play").is_err());
    }

    /// Ohne Rate sind die Frames zeitlos. Lieber absagen als schweigend mit 0
    /// rechnen.
    #[test]
    fn ohne_rate_ist_es_keine_mitschrift() {
        let text = "# musik-mitschrift 1\n# mitschnitt a.wav\n0 0.000 > do deck1.play";
        assert!(aus_text(text).is_err());
    }

    #[test]
    fn eine_neuere_fassung_wird_nicht_geraten() {
        let text = "# musik-mitschrift 99\n# rate 48000\n";
        let fehler = aus_text(text).expect_err("neuere Fassung wurde gelesen");
        assert!(fehler.contains("99"), "{fehler}");
    }

    #[test]
    fn geschriebenes_laesst_sich_wieder_lesen() {
        let ordner = std::env::temp_dir().join(format!("mitschrift-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Ordner");
        let pfad = ordner.join("set.mitschrift");

        {
            let mut m =
                Mitschrift::starten(&pfad, Path::new("/pfad/set.wav"), 48_000).expect("starten");
            m.halten(0, &[stand(0, 0.0)], Richtung::Befehl, "do deck1.play");
            m.halten(
                120_960,
                &[stand(0, 64.0)],
                Richtung::Befehl,
                "ramp master.crossfader 1 32",
            );
            m.halten(121_000, &[stand(0, 64.021)], Richtung::Meldung, "ok plan 3");
            assert_eq!(m.fehler(), 0);
        }

        let p = lesen(&pfad).expect("lesen");
        assert_eq!(p.kopf.mitschnitt, "/pfad/set.wav");
        assert_eq!(p.ereignisse.len(), 3);
        assert_eq!(p.ereignisse[2].richtung, Richtung::Meldung);
        assert_eq!(p.unlesbar, 0);

        std::fs::remove_dir_all(&ordner).ok();
    }

    /// Eine Antwort mit vierzig Treffern würde die Mitschrift zuschütten.
    #[test]
    fn eine_mehrzeilige_antwort_wird_gekuerzt() {
        let ordner = std::env::temp_dir().join(format!("mitschrift-lang-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Ordner");
        let pfad = ordner.join("set.mitschrift");

        {
            let mut m = Mitschrift::starten(&pfad, Path::new("set.wav"), 48_000).expect("starten");
            m.halten(0, &[], Richtung::Meldung, "ok 3 Treffer\neins\nzwei\ndrei");
        }

        let p = lesen(&pfad).expect("lesen");
        assert_eq!(p.ereignisse.len(), 1, "aus einer Antwort wurden mehrere");
        assert_eq!(p.ereignisse[0].text, "ok 3 Treffer … (+3 Zeilen)");

        std::fs::remove_dir_all(&ordner).ok();
    }

    #[test]
    fn eine_leere_antwort_schreibt_nichts() {
        let ordner = std::env::temp_dir().join(format!("mitschrift-leer-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).expect("Ordner");
        let pfad = ordner.join("set.mitschrift");

        {
            let mut m = Mitschrift::starten(&pfad, Path::new("set.wav"), 48_000).expect("starten");
            m.halten(0, &[], Richtung::Meldung, "   \n  ");
        }

        assert_eq!(lesen(&pfad).expect("lesen").ereignisse.len(), 0);
        std::fs::remove_dir_all(&ordner).ok();
    }
}
