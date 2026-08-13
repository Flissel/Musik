//! Tonart als Wert — Name, Camelot-Notation und harmonische Nachbarschaft.
//!
//! Hier steht nur das Vokabular, nicht das Erkennen. Das gehört nach
//! `analysis`, weil es eine FFT braucht und offline läuft. Der Wert dagegen
//! wird überall gebraucht: die Sammlung filtert danach, das Steuerpult gibt
//! ihn aus, die Oberfläche zeigt ihn an. Läge er in `analysis`, müssten alle
//! drei den Analyseapparat mitschleppen, nur um „Am" schreiben zu können —
//! dasselbe Argument, aus dem [`crate::Beatgrid`] hier liegt und nicht dort.

/// Die zwölf Halbtöne, beginnend bei C.
pub const TOENE: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tonart {
    /// 0 = C, 1 = C#, … 11 = B.
    pub grundton: u8,
    pub dur: bool,
}

impl Tonart {
    pub fn neu(grundton: u8, dur: bool) -> Tonart {
        Tonart {
            grundton: grundton % 12,
            dur,
        }
    }

    /// Übliche Schreibweise: `Am`, `F#`, `C`.
    pub fn name(&self) -> String {
        let ton = TOENE[self.grundton as usize % 12];
        if self.dur {
            ton.to_string()
        } else {
            format!("{ton}m")
        }
    }

    /// Camelot-Notation, nach der DJs mischen: `8A`, `5B`.
    ///
    /// Auf dem Rad liegen verwandte Tonarten nebeneinander. A ist Moll, B ist
    /// Dur; 8A (a-Moll) und 8B (C-Dur) sind Paralleltonarten.
    pub fn camelot(&self) -> String {
        let (zahl, seite) = self.camelot_teile();
        format!("{zahl}{seite}")
    }

    /// Zahl und Seite getrennt — für Vergleiche, die nicht über Text gehen
    /// sollen.
    pub fn camelot_teile(&self) -> (u8, char) {
        // Der Quintenzirkel in Camelot-Zählung. Index ist der Grundton.
        const DUR_ZAHL: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];
        const MOLL_ZAHL: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];

        let i = self.grundton as usize % 12;
        if self.dur {
            (DUR_ZAHL[i], 'B')
        } else {
            (MOLL_ZAHL[i], 'A')
        }
    }

    /// Liest `Am`, `F#`, `8A` oder `5B` zurück ein.
    pub fn parse(text: &str) -> Option<Tonart> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }

        // Camelot zuerst: fängt mit einer Ziffer an.
        if text.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let (zahl, buchstabe) = text.split_at(text.len() - 1);
            let zahl: u8 = zahl.parse().ok()?;
            let dur = match buchstabe {
                "B" | "b" => true,
                "A" | "a" => false,
                _ => return None,
            };
            return (0..12u8)
                .map(|g| Tonart { grundton: g, dur })
                .find(|t| t.camelot_teile() == (zahl, if dur { 'B' } else { 'A' }));
        }

        let moll = text.ends_with('m') && !text.ends_with("dim");
        let ton = if moll { &text[..text.len() - 1] } else { text };
        let grundton = TOENE.iter().position(|t| t.eq_ignore_ascii_case(ton))?;

        Some(Tonart {
            grundton: grundton as u8,
            dur: !moll,
        })
    }

    /// Ob sich zwei Tonarten harmonisch vertragen.
    ///
    /// Die Regel vom Camelot-Rad: dieselbe Zahl (Parallele), eine Zahl weiter
    /// oder zurück, oder dieselbe Tonart. Mehr ist eine Frage des Geschmacks
    /// und gehört nicht in eine Bibliothek.
    pub fn passt_zu(&self, andere: &Tonart) -> bool {
        let (za, sa) = self.camelot_teile();
        let (zb, sb) = andere.camelot_teile();

        if sa == sb {
            // Auf dem Rad ist nach 12 wieder 1.
            let abstand = (za as i32 - zb as i32).rem_euclid(12);
            abstand <= 1 || abstand >= 11
        } else {
            za == zb
        }
    }

    /// Alle Tonarten, die sich mit dieser vertragen — sie selbst eingeschlossen.
    ///
    /// Vier Stück: sie selbst, die Parallele, und die beiden Nachbarn auf dem
    /// Rad. Wer harmonisch sucht, braucht genau diese Liste, und sie hier
    /// auszurechnen ist verlässlicher, als sie an drei Stellen von Hand
    /// aufzuzählen.
    pub fn verwandte(&self) -> Vec<Tonart> {
        (0..12u8)
            .flat_map(|g| [Tonart::neu(g, true), Tonart::neu(g, false)])
            .filter(|t| self.passt_zu(t))
            .collect()
    }
}

impl std::fmt::Display for Tonart {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_und_name_gehen_hin_und_zurueck() {
        for grundton in 0..12u8 {
            for dur in [true, false] {
                let t = Tonart { grundton, dur };
                assert_eq!(Tonart::parse(&t.name()), Some(t), "Name {}", t.name());
                assert_eq!(
                    Tonart::parse(&t.camelot()),
                    Some(t),
                    "Camelot {}",
                    t.camelot()
                );
            }
        }
        assert_eq!(Tonart::parse("Quatsch"), None);
        assert_eq!(Tonart::parse(""), None);
    }

    #[test]
    fn die_camelot_zahlen_stimmen_mit_der_ueblichen_tafel() {
        // Stichproben aus dem Rad, wie es auf jedem Spickzettel steht.
        let paare = [
            ("C", "8B"),
            ("Am", "8A"),
            ("G", "9B"),
            ("Em", "9A"),
            ("F", "7B"),
            ("Dm", "7A"),
            ("A#", "6B"),
        ];
        for (name, camelot) in paare {
            let t = Tonart::parse(name).unwrap();
            assert_eq!(t.camelot(), camelot, "{name}");
        }
    }

    #[test]
    fn harmonisch_passt_was_auf_dem_rad_benachbart_ist() {
        let a_moll = Tonart::parse("Am").unwrap();

        // Parallele und Nachbarn.
        assert!(a_moll.passt_zu(&Tonart::parse("C").unwrap()), "8A zu 8B");
        assert!(a_moll.passt_zu(&Tonart::parse("Em").unwrap()), "8A zu 9A");
        assert!(a_moll.passt_zu(&Tonart::parse("Dm").unwrap()), "8A zu 7A");
        assert!(a_moll.passt_zu(&a_moll), "zu sich selbst");

        // Und was nicht passt, passt nicht.
        assert!(!a_moll.passt_zu(&Tonart::parse("D#m").unwrap()), "8A zu 2A");
        assert!(!a_moll.passt_zu(&Tonart::parse("F#").unwrap()), "8A zu 11B");
    }

    #[test]
    fn das_rad_schliesst_sich_bei_zwoelf() {
        // 12A und 1A sind Nachbarn, auch wenn die Zahlen weit auseinander
        // aussehen — ein Modulo-Fehler fiele genau hier auf.
        let zwoelf = Tonart::parse("12A").unwrap();
        let eins = Tonart::parse("1A").unwrap();
        assert!(
            zwoelf.passt_zu(&eins),
            "{} zu {}",
            zwoelf.camelot(),
            eins.camelot()
        );
    }

    #[test]
    fn die_verwandtschaft_hat_genau_vier_mitglieder() {
        let a_moll = Tonart::parse("Am").unwrap();
        let verwandte: Vec<String> = a_moll.verwandte().iter().map(|t| t.camelot()).collect();

        // Sie selbst, die Parallele und die beiden Nachbarn — nicht mehr.
        assert_eq!(verwandte.len(), 4, "{verwandte:?}");
        for erwartet in ["8A", "8B", "7A", "9A"] {
            assert!(verwandte.contains(&erwartet.to_string()), "{verwandte:?}");
        }

        // Und die Beziehung ist gegenseitig: Wer in meiner Liste steht, hat
        // mich in seiner. Sonst hinge das Suchergebnis davon ab, von welchem
        // Deck aus man fragt.
        for andere in a_moll.verwandte() {
            assert!(
                andere.passt_zu(&a_moll),
                "{} hat {} nicht zurück",
                andere.camelot(),
                a_moll.camelot()
            );
        }
    }

    #[test]
    fn parallele_tonarten_bleiben_mischbar_auch_wenn_man_sie_verwechselt() {
        // Dur und die parallele Molltonart bestehen aus denselben Tönen; ob
        // C-Dur oder a-Moll herauskommt, ist auch für Menschen manchmal
        // Auslegungssache. Beim Mischen ist das verschmerzbar — auf dem Rad
        // tragen beide dieselbe Zahl.
        let dur = Tonart::parse("C").unwrap();
        let moll = Tonart::parse("Am").unwrap();

        assert_ne!(dur, moll);
        assert_eq!(dur.camelot(), "8B");
        assert_eq!(moll.camelot(), "8A");
        assert!(
            dur.passt_zu(&moll),
            "eine Verwechslung der Parallelen darf beim Mischen nicht schaden"
        );
    }
}
