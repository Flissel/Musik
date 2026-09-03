//! Was als Nächstes kommt — das gemeinsame Blatt für die Auswahl.
//!
//! **Warum das nicht jeder Bediener für sich führen kann.** Sobald mehr als
//! einer auswählt, ist „was kommt als Nächstes" eine Frage an die Anlage und
//! nicht an einen Kopf. Zwei Agenten, die ihre Liste je für sich halten, legen
//! irgendwann beide auf dasselbe Deck; einer verliert, und niemand merkt es,
//! bis der falsche Track läuft.
//!
//! Deshalb liegt die Liste hier, im Pult, hinter demselben Mutex wie alles
//! andere. Wer abnimmt, nimmt den Eintrag heraus — ein zweiter, der im selben
//! Moment abnimmt, bekommt den nächsten und nicht denselben.
//!
//! **Jeder Eintrag trägt eine Notiz.** Das ist der Unterschied zu einer
//! Playlist: Wer einen Track vormerkt, weiß, warum — harmonisch passend, mehr
//! Druck, Ruhepunkt nach dem Peak. Ohne die Notiz muss der Nächste den Grund
//! aus BPM und Tonart erraten, und bei einem Team von Agenten heißt das: Er
//! erfindet ihn.

/// Ein vorgemerkter Track.
#[derive(Debug, Clone, PartialEq)]
pub struct Eintrag {
    /// Nummer, unter der sich der Eintrag ansprechen lässt. Bleibt vergeben,
    /// auch wenn davor etwas herausgenommen wird — anders als die Position.
    pub id: u64,
    pub pfad: String,
    /// Warum er hier steht. Leer erlaubt, aber schade.
    pub notiz: String,
}

#[derive(Default)]
pub struct Warteschlange {
    eintraege: Vec<Eintrag>,
    naechste_id: u64,
}

impl Warteschlange {
    pub fn neu() -> Warteschlange {
        Warteschlange::default()
    }

    pub fn eintraege(&self) -> &[Eintrag] {
        &self.eintraege
    }

    pub fn ist_leer(&self) -> bool {
        self.eintraege.is_empty()
    }

    /// Hängt hinten an.
    ///
    /// `Err(nr)` heißt: Der Pfad steht schon unter dieser Nummer. Das ist kein
    /// Formfehler, sondern der häufigste Zusammenstoß, wenn zwei unabhängig
    /// voneinander auswählen — beide finden denselben passenden Track. Ihn
    /// stillschweigend zweimal aufzunehmen hieße, ihn auch zweimal zu spielen.
    pub fn anhaengen(&mut self, pfad: String, notiz: String) -> Result<u64, u64> {
        if let Some(schon) = self.eintraege.iter().find(|e| e.pfad == pfad) {
            return Err(schon.id);
        }
        self.naechste_id += 1;
        self.eintraege.push(Eintrag {
            id: self.naechste_id,
            pfad,
            notiz,
        });
        Ok(self.naechste_id)
    }

    /// Nimmt den vordersten heraus.
    pub fn abnehmen(&mut self) -> Option<Eintrag> {
        if self.eintraege.is_empty() {
            return None;
        }
        Some(self.eintraege.remove(0))
    }

    /// Legt einen abgenommenen Eintrag wieder nach vorn.
    ///
    /// Für den Fall, dass das Auflegen scheitert. Ein Eintrag, der aus der
    /// Liste verschwindet, ohne gespielt zu werden, fällt niemandem auf — bis
    /// er fehlt.
    pub fn zuruecklegen(&mut self, eintrag: Eintrag) {
        self.naechste_id = self.naechste_id.max(eintrag.id);
        self.eintraege.insert(0, eintrag);
    }

    pub fn streichen(&mut self, id: u64) -> Option<Eintrag> {
        let stelle = self.eintraege.iter().position(|e| e.id == id)?;
        Some(self.eintraege.remove(stelle))
    }

    /// Schreibt die Notiz neu — auch, um eine Einschätzung zu widerrufen.
    pub fn notieren(&mut self, id: u64, notiz: String) -> bool {
        match self.eintraege.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.notiz = notiz;
                true
            }
            None => false,
        }
    }

    /// Zieht einen Eintrag nach vorn — „der als Nächstes".
    pub fn vorziehen(&mut self, id: u64) -> bool {
        match self.eintraege.iter().position(|e| e.id == id) {
            Some(stelle) => {
                let e = self.eintraege.remove(stelle);
                self.eintraege.insert(0, e);
                true
            }
            None => false,
        }
    }

    /// Leert die Liste und sagt, wie viele es waren.
    pub fn leeren(&mut self) -> usize {
        let weg = self.eintraege.len();
        self.eintraege.clear();
        weg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wl() -> Warteschlange {
        let mut w = Warteschlange::neu();
        w.anhaengen("/a.mp3".into(), "warm".into()).unwrap();
        w.anhaengen("/b.mp3".into(), String::new()).unwrap();
        w.anhaengen("/c.mp3".into(), "Peak".into()).unwrap();
        w
    }

    #[test]
    fn abgenommen_wird_von_vorn() {
        let mut w = wl();
        assert_eq!(w.abnehmen().unwrap().pfad, "/a.mp3");
        assert_eq!(w.abnehmen().unwrap().pfad, "/b.mp3");
        assert_eq!(w.eintraege().len(), 1);
    }

    /// Der häufigste Zusammenstoß zweier Auswählender.
    ///
    /// Beide suchen, was zu 128 BPM in 8A passt, beide finden denselben Track.
    /// Ihn zweimal aufzunehmen hieße, ihn zweimal zu spielen.
    #[test]
    fn derselbe_track_kommt_nicht_zweimal_hinein() {
        let mut w = wl();
        assert_eq!(w.anhaengen("/b.mp3".into(), "passt".into()), Err(2));
        assert_eq!(w.eintraege().len(), 3);

        // Nach dem Abnehmen ist der Platz wieder frei — ein Track darf später
        // erneut vorgemerkt werden.
        while w.abnehmen().is_some() {}
        assert!(w.anhaengen("/b.mp3".into(), String::new()).is_ok());
    }

    #[test]
    fn nummern_bleiben_stehen_wenn_davor_etwas_verschwindet() {
        // Sonst spräche ein Agent, der sich Nummer 3 gemerkt hat, plötzlich
        // über einen anderen Track.
        let mut w = wl();
        w.abnehmen();
        let dritter = w.eintraege().iter().find(|e| e.id == 3).expect("Nummer 3");
        assert_eq!(dritter.pfad, "/c.mp3");
    }

    #[test]
    fn vorziehen_macht_einen_zum_naechsten() {
        let mut w = wl();
        assert!(w.vorziehen(3));
        assert_eq!(w.abnehmen().unwrap().pfad, "/c.mp3");
        // Die Reihenfolge dahinter bleibt, wie sie war.
        assert_eq!(w.abnehmen().unwrap().pfad, "/a.mp3");
    }

    #[test]
    fn eine_notiz_laesst_sich_nachtragen_und_widerrufen() {
        let mut w = wl();
        assert!(w.notieren(2, "doch zu ruhig".into()));
        assert_eq!(w.eintraege()[1].notiz, "doch zu ruhig");
        assert!(!w.notieren(99, "gibt es nicht".into()));
    }

    #[test]
    fn streichen_und_leeren_melden_was_sie_getan_haben() {
        let mut w = wl();
        assert_eq!(w.streichen(2).unwrap().pfad, "/b.mp3");
        assert!(w.streichen(2).is_none());
        assert_eq!(w.leeren(), 2);
        assert!(w.ist_leer());
    }
}
