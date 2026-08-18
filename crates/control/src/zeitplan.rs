//! Der Zeitplan: Was später geschehen soll, und über wie viele Beats.
//!
//! **Warum das nicht der Aufrufer selbst machen kann.** Ein Übergang ist keine
//! Folge von Reglerstellungen, sondern eine Bewegung über Takte: „Bass raus
//! über acht Beats, dann Crossfader rüber über sechzehn". Wer das von außen
//! nachbaut, müsste in einer engen Schleife `beat_phase` pollen und zwischen
//! den Schritten schlafen — über einen Socket, dessen Timing dem Scheduler des
//! Betriebssystems ausgeliefert ist. Das Ergebnis eiert hörbar, und der Agent
//! ist die ganze Zeit blockiert.
//!
//! Hier drin läuft es auf dem **Beatgrid** statt auf der Uhr. Ein Plan, der in
//! Sekunden rechnet, geht schief, sobald jemand am Tempo dreht; einer, der in
//! Beats rechnet, bleibt musikalisch richtig. Steht das Deck, steht auch der
//! Plan — was gestoppt ist, hat keine Takte.
//!
//! Neben „in so vielen Beats" gibt es „sobald es so weit ist": Ein [`Was::Wenn`]
//! hängt nicht an Takten, sondern an einem Wert — „wenn Deck A noch 32 Beats
//! hat, leg den nächsten auf". Ohne das müsste ein Bediener `beats_left`
//! abonnieren und bekäme zwanzig Zahlen je Sekunde, aus denen er die eine
//! Schwelle selbst heraussucht. Über eine Chat-Schnittstelle ist das nicht nur
//! unbequem, sondern unbezahlbar.
//!
//! **Für ein Team von Agenten ist der Plan zugleich das gemeinsame Blatt.**
//! Wer `plan` liest, sieht, was die anderen vorhaben, statt es aus
//! Reglerbewegungen zu erraten. Und eine Rampe gibt auf, sobald jemand anders
//! denselben Regler anfasst — siehe [`Rampe`].

use std::sync::{Arc, Mutex};

use crate::pult::{Fehler, Steuerpult};
use crate::schluessel::{Gruppe, Schluessel};
use crate::wert::Wert;

/// Wie weit ein Reglerwert von dem abweichen darf, was die Rampe zuletzt
/// geschrieben hat, bevor sie sich für abgelöst hält.
///
/// Reglerwerte laufen als `f32` durch die Kommandoschlange und kommen als
/// `f64` zurück; ein exakter Vergleich fände deshalb ständig Unterschiede, die
/// keine sind. Alles darüber ist ein fremder Eingriff.
const ABGELOEST: f64 = 1e-4;

/// Ein Auftrag im Plan.
#[derive(Debug, Clone)]
pub struct Auftrag {
    pub id: u64,
    /// Auf wessen Takten der Auftrag liegt.
    ///
    /// Bei einem [`Was::Wenn`] ohne Bedeutung — das hängt an einem Wert und
    /// nicht an Takten.
    pub takt_deck: usize,
    /// Beat des Taktgeber-Decks, ab dem gehandelt wird.
    pub ab_beat: f64,
    /// Der zuletzt gesehene Beat des Taktgebers.
    ///
    /// Nur, um einen **Rücksprung** zu bemerken: Läuft das Deck in einer
    /// Schleife oder springt jemand zurück, zählt sein Beat wieder von vorn,
    /// und dann ist alles falsch, was daran hängt — eine Rampe fängt von vorn
    /// an und wiederholt sich endlos, ein Vorgemerktes kommt nie an. Beides
    /// hat kein Test gezeigt, sondern ein Blick auf den Crossfader, der sieben
    /// Mal hin und her fuhr.
    pub zuletzt_beat: f64,
    pub was: Was,
}

/// Wie weit der Beat zurückgehen darf, bevor es ein Sprung ist.
///
/// Ein halber Beat: Die Position wird aus der Frame-Zahl gerechnet und
/// schwankt um Bruchteile, aber sie geht nicht rückwärts. Alles darüber ist
/// eine Schleife oder ein Sprung.
pub const RUECKSPRUNG: f64 = 0.5;

#[derive(Debug, Clone)]
pub enum Was {
    /// Ein Reglerwert, der über mehrere Beats wandert.
    Rampe(Rampe),
    /// Eine fertige Protokollzeile, die später ausgeführt wird.
    Spaeter { beim_beat: f64, zeile: String },
    /// Dieselbe Zeile, aber ausgelöst von einem Wert statt von der Zeit.
    Wenn {
        control: Schluessel,
        vergleich: Vergleich,
        schwelle: f64,
        zeile: String,
    },
}

/// In welche Richtung eine Schwelle überschritten wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vergleich {
    Unter,
    Ueber,
}

impl Vergleich {
    pub fn parse(text: &str) -> Option<Vergleich> {
        match text {
            "<" => Some(Vergleich::Unter),
            ">" => Some(Vergleich::Ueber),
            _ => None,
        }
    }

    pub fn zeichen(&self) -> &'static str {
        match self {
            Vergleich::Unter => "<",
            Vergleich::Ueber => ">",
        }
    }

    fn trifft_zu(&self, wert: f64, schwelle: f64) -> bool {
        match self {
            Vergleich::Unter => wert < schwelle,
            Vergleich::Ueber => wert > schwelle,
        }
    }
}

/// Wie sich eine Rampe über ihre Strecke verteilt.
///
/// **Dieselbe Strecke, andere Wirkung.** Ein Fader, der gleichmäßig wandert,
/// klingt nach Automat; einer, der anfängt zu ziehen, durchzieht und wieder
/// abflacht, klingt nach einer Hand. Der Weg ist in beiden Fällen derselbe —
/// nur die Verteilung über die Beats ist eine andere, und die hört man.
///
/// Die Namen sagen, **wann** die Bewegung passiert, nicht welche Kurve
/// dahintersteckt: Danach fragt man beim Auflegen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Form {
    /// Gleichmäßig. Ehrlich, unauffällig, für Bass-Swaps und alles, was nicht
    /// als Geste gemeint ist.
    #[default]
    Linear,
    /// Langsam los, durchziehen, weich ankommen — die S-Kurve.
    ///
    /// Für lange Blenden die Voreinstellung von Hand: Der Anfang bleibt
    /// unauffällig, die Mitte ist entschieden, das Ende schleicht sich nicht
    /// an, sondern legt sich hin.
    Weich,
    /// Lange fast nichts, dann schnell.
    ///
    /// Hält den ausgehenden Track so lange wie möglich präsent. Wer ein Outro
    /// auskosten will und trotzdem pünktlich fertig sein muss, nimmt das.
    Spaet,
    /// Sofort viel, dann auslaufen.
    ///
    /// Bringt den eingehenden Track schnell in den Raum und lässt den Rest
    /// nachlaufen — für einen Wechsel, der als Ereignis gemeint ist.
    Frueh,
}

impl Form {
    pub fn name(&self) -> &'static str {
        match self {
            Form::Linear => "linear",
            Form::Weich => "weich",
            Form::Spaet => "spaet",
            Form::Frueh => "frueh",
        }
    }

    pub fn parse(text: &str) -> Option<Form> {
        match text {
            "linear" => Some(Form::Linear),
            "weich" => Some(Form::Weich),
            "spaet" => Some(Form::Spaet),
            "frueh" => Some(Form::Frueh),
            _ => None,
        }
    }

    pub const ALLE: [Form; 4] = [Form::Linear, Form::Weich, Form::Spaet, Form::Frueh];

    /// Bildet den Anteil der Strecke auf den Anteil des Weges ab.
    ///
    /// Beide zwischen 0 und 1, und **jede Form muss 0 auf 0 und 1 auf 1
    /// abbilden**: Sonst spränge der Regler beim Anfang oder käme nicht an.
    pub fn anwenden(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Form::Linear => t,
            // Smoothstep. Die Ableitung ist an beiden Enden null — deshalb
            // kein hörbarer Ruck beim Losfahren und keiner beim Ankommen.
            Form::Weich => t * t * (3.0 - 2.0 * t),
            Form::Spaet => t * t,
            Form::Frueh => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// Eine Bewegung eines Reglers über Beats.
///
/// **Sie gibt auf, sobald jemand anders denselben Regler anfasst.** Ohne das
/// wäre die Rampe stärker als der Mensch daneben: Er zieht den Fader zu, und
/// eine halbe Sekunde später steht er wieder offen. Geprüft wird nicht über
/// eine Kennung, sondern über den Wert selbst — steht dort etwas anderes, als
/// die Rampe zuletzt geschrieben hat, war jemand anders am Werk. Das gilt für
/// einen Menschen an der Oberfläche genauso wie für einen zweiten Agenten.
#[derive(Debug, Clone)]
pub struct Rampe {
    pub control: Schluessel,
    pub von: f64,
    pub nach: f64,
    /// Länge in Beats des Taktgeber-Decks.
    pub beats: f64,
    /// Was zuletzt geschrieben wurde — der Prüfstein für fremde Eingriffe.
    pub zuletzt: f64,
    /// Wie sich die Bewegung über die Beats verteilt.
    pub form: Form,
}

/// Was mit einem Auftrag beim Takt geschah — nur für die Meldung nach außen.
#[derive(Debug, PartialEq)]
pub enum Ausgang {
    Laeuft,
    Fertig,
    /// Jemand anders hat den Regler angefasst.
    Abgeloest,
    /// Der Taktgeber hat kein Grid mehr, oder das Deck ist weg.
    Haltlos,
}

#[derive(Default)]
pub struct Zeitplan {
    auftraege: Vec<Auftrag>,
    naechste_id: u64,
}

impl Zeitplan {
    pub fn neu() -> Zeitplan {
        Zeitplan::default()
    }

    pub fn auftraege(&self) -> &[Auftrag] {
        &self.auftraege
    }

    pub fn ist_leer(&self) -> bool {
        self.auftraege.is_empty()
    }

    fn aufnehmen(&mut self, takt_deck: usize, ab_beat: f64, was: Was) -> u64 {
        self.naechste_id += 1;
        self.auftraege.push(Auftrag {
            id: self.naechste_id,
            takt_deck,
            ab_beat,
            zuletzt_beat: ab_beat,
            was,
        });
        self.naechste_id
    }

    /// Übernimmt den Zählerstand eines anderen Plans.
    ///
    /// Der Taktgeber nimmt den Plan aus dem Pult heraus, arbeitet ihn ab und
    /// legt ihn zurück. Was währenddessen neu vorgemerkt wird — ein `in 16
    /// ramp …`, das gerade fällig geworden ist —, landet im leeren Plan, der
    /// solange im Pult liegt, und finge dort wieder bei 1 an. Damit trüge ein
    /// frischer Auftrag dieselbe Nummer wie ein noch laufender, und `cancel 1`
    /// träfe beide.
    pub fn zaehler_von(&mut self, anderer: &Zeitplan) {
        self.naechste_id = self.naechste_id.max(anderer.naechste_id);
    }

    /// Nimmt einen fertigen Auftrag auf — für den Taktgeber, der den Plan
    /// kurzzeitig aus dem Pult nimmt und zurücklegt.
    pub fn uebernehmen(&mut self, auftrag: Auftrag) {
        self.naechste_id = self.naechste_id.max(auftrag.id);
        self.auftraege.push(auftrag);
    }

    /// Nimmt einen Auftrag zurück. `None` löscht alle.
    ///
    /// Ein Agent muss zurücknehmen können, was er vorgemerkt hat — sonst wäre
    /// jeder Plan eine Einbahnstraße, und bei mehreren Bedienern erst recht.
    pub fn streichen(&mut self, id: Option<u64>) -> usize {
        let vorher = self.auftraege.len();
        match id {
            Some(id) => self.auftraege.retain(|a| a.id != id),
            None => self.auftraege.clear(),
        }
        vorher - self.auftraege.len()
    }
}

/// Wo ein Deck gerade auf seinem Beatgrid steht.
///
/// `None`, wenn es kein Grid hat — dann gibt es keine Takte, auf die sich ein
/// Plan beziehen könnte.
pub fn beat_jetzt(pult: &Steuerpult, deck: usize) -> Option<f64> {
    let d = pult.decks().get(deck)?;
    let grid = d.state.grid()?;
    Some(grid.beat_at(d.state.position_frames() as f64, d.sample_rate))
}

/// Welches Deck den Takt für ein Control vorgibt.
///
/// Ein Kanalzug erbt ihn vom Deck, das auf ihm liegt; ein Deck ist sein
/// eigener Taktgeber. Für die Summe gibt es keine natürliche Antwort — dort
/// zählt das erste Deck mit Grid, und wer etwas anderes will, sagt es dazu.
pub fn taktgeber(pult: &Steuerpult, control: &Schluessel) -> Option<usize> {
    match control.gruppe {
        Gruppe::Deck(i) => (i < pult.decks().len()).then_some(i),
        Gruppe::Kanal(k) => pult.decks().iter().position(|d| d.kanal == k),
        Gruppe::Master => (0..pult.decks().len()).find(|i| beat_jetzt(pult, *i).is_some()),
    }
}

/// Führt aus, was fällig ist.
///
/// Gibt für jeden erledigten oder abgebrochenen Auftrag eine Zeile zurück —
/// dieselbe Form wie sonst im Protokoll, damit ein Abonnent sie mitliest.
pub fn takt(
    pult: &mut Steuerpult,
    plan: &mut Zeitplan,
    ausfuehren: &mut dyn FnMut(&mut Steuerpult, &str) -> String,
) -> Vec<String> {
    let mut meldungen = Vec::new();
    // Was ein fälliger Befehl selbst vormerkt, landet im Plan des Pults —
    // der ist gerade leer, weil der Taktgeber diesen hier herausgenommen hat.
    // Ohne den Zählerstand finge er wieder bei 1 an.
    pult.plan.zaehler_von(plan);
    // Herausnehmen, abarbeiten, zurücklegen: Sonst läge der Plan geborgt da,
    // während die Aufträge selbst wieder ins Pult schreiben.
    let mut offen = std::mem::take(&mut plan.auftraege);
    let mut bleibt = Vec::with_capacity(offen.len());

    for mut auftrag in offen.drain(..) {
        // Ein Wenn zuerst und getrennt: Es hängt an einem Wert, nicht an
        // Takten, und dürfte deshalb nicht daran scheitern, dass irgendein
        // Deck gerade kein Grid hat.
        if let Was::Wenn {
            control,
            vergleich,
            schwelle,
            zeile,
        } = &auftrag.was
        {
            let (control, vergleich, schwelle, zeile) =
                (control.clone(), *vergleich, *schwelle, zeile.clone());

            match pult.lies(&control) {
                Ok(Wert::Zahl(steht)) if vergleich.trifft_zu(steht, schwelle) => {
                    let antwort = ausfuehren(pult, &zeile);
                    meldungen.push(format!("plan {} ausgefuehrt {antwort}", auftrag.id));
                }
                // Noch nicht so weit — oder noch nicht bekannt. `Leer` heißt
                // nicht „nie": Ein Track, der gerade lädt, hat noch kein Grid
                // und damit noch keine Restbeats.
                Ok(_) => bleibt.push(auftrag),
                Err(e) => meldungen.push(format!("plan {} abgebrochen — {e}", auftrag.id)),
            }
            continue;
        }

        let Some(jetzt) = beat_jetzt(pult, auftrag.takt_deck) else {
            meldungen.push(format!(
                "plan {} abgebrochen — deck{} hat kein Grid mehr",
                auftrag.id,
                auftrag.takt_deck + 1
            ));
            continue;
        };

        // Ist der Taktgeber zurückgesprungen, ist die Grundlage des Auftrags
        // weg. Ihn trotzdem weiterlaufen zu lassen hieße, eine Rampe von vorn
        // beginnen zu lassen — bei einer Schleife immer wieder. Lieber
        // abbrechen und es sagen: Wer eine Schleife setzt, soll erfahren, dass
        // sein Plan daran hängt.
        if jetzt + RUECKSPRUNG < auftrag.zuletzt_beat {
            meldungen.push(format!(
                "plan {} abgebrochen — deck{} ist zurückgesprungen (Schleife oder Sprung)",
                auftrag.id,
                auftrag.takt_deck + 1
            ));
            continue;
        }
        auftrag.zuletzt_beat = jetzt;

        match &mut auftrag.was {
            Was::Spaeter { beim_beat, zeile } => {
                if jetzt + 1e-9 < *beim_beat {
                    bleibt.push(auftrag);
                    continue;
                }
                let antwort = ausfuehren(pult, &zeile.clone());
                meldungen.push(format!("plan {} ausgefuehrt {antwort}", auftrag.id));
            }
            // Oben schon behandelt; hier kommt nichts mehr an.
            Was::Wenn { .. } => unreachable!("Wenn wird vor dem Taktgeber behandelt"),
            Was::Rampe(rampe) => match rampe_schritt(pult, rampe, auftrag.ab_beat, jetzt) {
                Ausgang::Laeuft => bleibt.push(auftrag),
                Ausgang::Fertig => meldungen.push(format!(
                    "plan {} fertig {} {:.4}",
                    auftrag.id, rampe.control, rampe.nach
                )),
                Ausgang::Abgeloest => meldungen.push(format!(
                    "plan {} abgeloest {} — jemand anders hat den Regler",
                    auftrag.id, rampe.control
                )),
                Ausgang::Haltlos => {
                    meldungen.push(format!("plan {} abgebrochen {}", auftrag.id, rampe.control))
                }
            },
        }
    }

    plan.auftraege = bleibt;
    // In die Mitschrift gehört vor allem das hier: `plan 3 fertig` sagt auf
    // den Frame genau, wann eine Blende zu Ende war. Aus dem Klang allein wäre
    // das Ende einer langen Blende genauso schwer zu finden wie ihr Anfang —
    // dort ist der eingehende Track längst der einzige.
    for m in &meldungen {
        pult.halten(crate::mitschrift::Richtung::Meldung, m);
        // Und in den Ring, damit sie noch im selben Takt jemanden erreichen.
        // Die Mitschrift ist für hinterher; wer gerade auflegt, muss *jetzt*
        // erfahren, dass seine Blende abgelöst wurde.
        pult.ereignisse.melden(m);
    }
    meldungen
}

fn rampe_schritt(pult: &mut Steuerpult, rampe: &mut Rampe, ab: f64, jetzt: f64) -> Ausgang {
    // Noch nicht losgelaufen? Dann auch noch nichts anfassen.
    if jetzt + 1e-9 < ab {
        return Ausgang::Laeuft;
    }

    // Hat jemand anders den Regler bewegt, seit wir zuletzt geschrieben haben?
    let Ok(Wert::Zahl(steht)) = pult.lies(&rampe.control) else {
        return Ausgang::Haltlos;
    };
    if (steht - rampe.zuletzt).abs() > ABGELOEST {
        return Ausgang::Abgeloest;
    }

    let anteil = if rampe.beats > 0.0 {
        ((jetzt - ab) / rampe.beats).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let wert = rampe.von + (rampe.nach - rampe.von) * rampe.form.anwenden(anteil);

    // Nur um diesen einen Schreibvorgang herum: Das Pult kann sonst den letzten
    // Schritt einer Blende nicht von einem Schnitt unterscheiden.
    pult.rampe_am_werk = Some(format!("{}/{:.0}", rampe.form.name(), rampe.beats));
    let geschrieben = pult.schreibe(&rampe.control, Wert::Zahl(wert));
    pult.rampe_am_werk = None;
    if geschrieben.is_err() {
        return Ausgang::Haltlos;
    }
    // Zurücklesen statt den gewünschten Wert zu merken: Das Pult begrenzt, und
    // ein begrenzter Wert wäre sonst beim nächsten Takt ein „fremder Eingriff".
    rampe.zuletzt = match pult.lies(&rampe.control) {
        Ok(Wert::Zahl(v)) => v,
        _ => wert,
    };

    if anteil >= 1.0 {
        Ausgang::Fertig
    } else {
        Ausgang::Laeuft
    }
}

/// Merkt eine Rampe vor.
pub fn rampe_planen(
    pult: &Steuerpult,
    plan: &mut Zeitplan,
    control: Schluessel,
    nach: f64,
    beats: f64,
    takt_deck: Option<usize>,
    form: Form,
) -> Result<u64, Fehler> {
    if beats < 0.0 {
        return Err(Fehler::Argument {
            control: control.to_string(),
            erwartet: "eine Länge in Beats, nicht negativ".into(),
        });
    }

    let Ok(Wert::Zahl(von)) = pult.lies(&control) else {
        return Err(Fehler::FalscherTyp {
            control: control.to_string(),
            erwartet: crate::wert::Art::Zahl,
        });
    };

    let deck = takt_deck
        .or_else(|| taktgeber(pult, &control))
        .ok_or_else(|| Fehler::Gescheitert("kein Deck mit Beatgrid als Taktgeber".into()))?;
    let jetzt = beat_jetzt(pult, deck)
        .ok_or_else(|| Fehler::Gescheitert(format!("deck{} hat kein Beatgrid", deck + 1)))?;

    Ok(plan.aufnehmen(
        deck,
        jetzt,
        Was::Rampe(Rampe {
            control,
            von,
            nach,
            beats,
            zuletzt: von,
            form,
        }),
    ))
}

/// Merkt eine Protokollzeile für später vor.
pub fn spaeter_planen(
    pult: &Steuerpult,
    plan: &mut Zeitplan,
    beats: f64,
    zeile: String,
    takt_deck: Option<usize>,
) -> Result<u64, Fehler> {
    if beats < 0.0 {
        return Err(Fehler::Argument {
            control: "in".into(),
            erwartet: "eine Wartezeit in Beats, nicht negativ".into(),
        });
    }

    let deck = takt_deck
        .or_else(|| (0..pult.decks().len()).find(|i| beat_jetzt(pult, *i).is_some()))
        .ok_or_else(|| Fehler::Gescheitert("kein Deck mit Beatgrid als Taktgeber".into()))?;
    let jetzt = beat_jetzt(pult, deck)
        .ok_or_else(|| Fehler::Gescheitert(format!("deck{} hat kein Beatgrid", deck + 1)))?;

    Ok(plan.aufnehmen(
        deck,
        jetzt,
        Was::Spaeter {
            beim_beat: jetzt + beats,
            zeile,
        },
    ))
}

/// Merkt eine Protokollzeile für den Moment vor, in dem ein Wert eine Schwelle
/// überschreitet.
///
/// Geprüft wird beim Vormerken, ob das Control überhaupt eine Zahl ist — ein
/// Schalter oder ein Titel ließe sich nie mit einer Schwelle vergleichen, und
/// ein Auftrag, der stumm für immer wartet, ist schlimmer als eine Absage.
///
/// **Trifft die Bedingung schon jetzt zu, läuft der Befehl beim nächsten Takt.**
/// Das ist gewollt: `when` heißt „sobald es so weit ist", nicht „beim nächsten
/// Überschreiten". Wer auf eine Flanke warten will, prüft vorher selbst.
pub fn wenn_planen(
    pult: &Steuerpult,
    plan: &mut Zeitplan,
    control: Schluessel,
    vergleich: Vergleich,
    schwelle: f64,
    zeile: String,
) -> Result<u64, Fehler> {
    let Some(b) = pult.beschreibung(&control) else {
        return Err(Fehler::UnbekanntesControl(control.to_string()));
    };
    if b.art != crate::wert::Art::Zahl {
        return Err(Fehler::FalscherTyp {
            control: control.to_string(),
            erwartet: crate::wert::Art::Zahl,
        });
    }

    Ok(plan.aufnehmen(
        0,
        0.0,
        Was::Wenn {
            control,
            vergleich,
            schwelle,
            zeile,
        },
    ))
}

/// Wie oft der Plan nachsieht, ob etwas fällig ist.
///
/// 5 ms sind bei 128 BPM rund ein Prozent eines Beats — für eine Blende
/// unhörbar, für einen harten Schnitt auf die Eins gerade noch vertretbar.
/// Kürzer zu takten hieße, öfter den Mutex des Pults zu nehmen, an dem auch
/// die Oberfläche hängt. Sample-genau ist das ausdrücklich **nicht**: Dafür
/// müssten die Befehle im Audio-Callback liegen, und dorthin gehört keine
/// Reglerlogik.
pub const TAKT: std::time::Duration = std::time::Duration::from_millis(5);

/// Läuft, solange dieser Griff lebt.
pub struct Taktgeberthread {
    weiter: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Taktgeberthread {
    fn drop(&mut self) {
        self.weiter
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Startet den Takt, der den Plan abarbeitet.
///
/// Gehört bewusst nicht in den Server: Der Plan muss auch dann laufen, wenn
/// niemand über den Socket verbunden ist — die Oberfläche kann genauso eine
/// Rampe auslösen.
pub fn takt_starten(pult: Arc<Mutex<Steuerpult>>) -> Taktgeberthread {
    let weiter = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let laufen = Arc::clone(&weiter);

    std::thread::Builder::new()
        .name("zeitplan".into())
        .spawn(move || {
            while laufen.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(mut p) = pult.lock() {
                    if !p.plan.ist_leer() {
                        let mut plan = std::mem::take(&mut p.plan);
                        takt(&mut p, &mut plan, &mut |pult, zeile| {
                            let mut sitzung = crate::Sitzung::neu();
                            crate::behandle(pult, &mut sitzung, zeile)
                        });
                        // Was währenddessen dazukam, geht nicht verloren.
                        let neu = std::mem::take(&mut p.plan);
                        p.plan = plan;
                        for a in neu.auftraege() {
                            p.plan.uebernehmen(a.clone());
                        }
                    }
                }
                std::thread::sleep(TAKT);
            }
        })
        .expect("Zeitplan-Thread ließ sich nicht starten");

    Taktgeberthread { weiter }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{pult_mit_zwei_decks, rendern, RATE};
    use crate::wert::Wert;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    /// Führt den Plan aus, während das Deck spielt.
    fn laufen(
        pult: &mut Steuerpult,
        plan: &mut Zeitplan,
        runner: &mut audio_engine::EngineRunner,
        frames: usize,
    ) -> Vec<String> {
        let mut meldungen = Vec::new();
        // In Häppchen, damit die Rampe unterwegs mehrfach greift — genau wie
        // der Taktgeber-Thread im Betrieb.
        for _ in 0..20 {
            rendern(runner, frames / 20);
            meldungen.extend(takt(pult, plan, &mut |_, _| String::new()));
        }
        meldungen
    }

    /// Eine Blende über acht Beats — der eigentliche Zweck.
    #[test]
    fn eine_rampe_wandert_ueber_die_beats() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            8.0,
            None,
            Form::Linear,
        )
        .expect("planen");

        // Bei 128 BPM sind acht Beats 3,75 s. Erst die Hälfte.
        laufen(
            &mut pult,
            &mut plan,
            &mut runner,
            (RATE as f64 * 1.875) as usize,
        );
        let Wert::Zahl(mitte) = pult.lies(&k("channel1.fader")).unwrap() else {
            panic!()
        };
        assert!(
            (0.35..0.65).contains(&mitte),
            "nach der Hälfte steht der Fader bei {mitte:.3}, nicht in der Mitte"
        );
        assert!(!plan.ist_leer(), "die Rampe ist zu früh fertig");

        // Und zu Ende.
        let meldungen = laufen(
            &mut pult,
            &mut plan,
            &mut runner,
            (RATE as f64 * 2.2) as usize,
        );
        assert_eq!(pult.lies(&k("channel1.fader")).unwrap(), Wert::Zahl(0.0));
        assert!(plan.ist_leer(), "die Rampe hängt noch im Plan");
        assert!(
            meldungen.iter().any(|m| m.contains("fertig")),
            "{meldungen:?}"
        );
    }

    /// Zwei offene Kanäle, zwei laufende Decks — so sieht jeder Griff aus,
    /// bevor der Crossfader sich bewegt.
    fn aufgelegt(pult: &mut Steuerpult) {
        for z in ["channel1.fader", "channel2.fader"] {
            pult.schreibe(&k(z), Wert::Zahl(1.0)).unwrap();
        }
        for d in ["deck1.play", "deck2.play"] {
            pult.schreibe(&k(d), Wert::Schalter(true)).unwrap();
        }
    }

    /// Den Fader vor dem Set auf eine Seite zu stellen ist kein Übergang.
    ///
    /// Gefunden am laufenden Programm, nicht im Test: Nach `set channel1.fader
    /// 1; set master.crossfader -1; set deck1.play 1` stand dort bereits
    /// `transitions schnitt`. Niemand hatte übergeblendet — es lief ja erst
    /// ein Deck. Eine Zurückhaltung, die schon beim Einrichten mitzählt,
    /// warnt beim ersten echten Griff vor einer Wiederholung, die es nicht gab.
    #[test]
    fn den_fader_vor_dem_set_zu_parken_ist_kein_uebergang() {
        let (mut pult, _runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();

        // Erst der Fader, dann das Deck — die Reihenfolge aus dem Fund.
        pult.schreibe(&k("master.crossfader"), Wert::Zahl(-1.0))
            .unwrap();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();
        assert_eq!(pult.lies(&k("master.transitions")).unwrap(), Wert::Leer);

        // Und andersherum, denn so richtet man eher ein: Deck läuft, dann wird
        // der Fader dorthin gestellt. Ein einzelnes laufendes Deck reicht
        // nicht — sonst zählte genau dieser Griff.
        pult.schreibe(&k("master.crossfader"), Wert::Zahl(0.0))
            .unwrap();
        pult.schreibe(&k("master.crossfader"), Wert::Zahl(-1.0))
            .unwrap();

        assert_eq!(pult.lies(&k("master.repeats")).unwrap(), Wert::Zahl(0.0));
        assert_eq!(pult.lies(&k("master.transitions")).unwrap(), Wert::Leer);
    }

    /// Eine Blende ist **ein** Übergang, keine achtzig.
    ///
    /// Eine Rampe schreibt bei jedem Takt in denselben Regler. Zählte man den
    /// Wert statt den Seitenwechsel, stünde nach einer einzigen Blende eine
    /// Wiederholung von 80 da — und die Zahl, die vor Eintönigkeit warnen soll,
    /// wäre selbst der Grund, sie zu ignorieren.
    #[test]
    fn eine_blende_zaehlt_einmal_nicht_achtzigmal() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        aufgelegt(&mut pult);

        rampe_planen(
            &pult,
            &mut plan,
            k("master.crossfader"),
            1.0,
            8.0,
            None,
            Form::Weich,
        )
        .expect("planen");
        laufen(
            &mut pult,
            &mut plan,
            &mut runner,
            (RATE as f64 * 4.2) as usize,
        );

        assert_eq!(pult.lies(&k("master.crossfader")).unwrap(), Wert::Zahl(1.0));
        assert_eq!(pult.lies(&k("master.repeats")).unwrap(), Wert::Zahl(1.0));
        assert_eq!(
            pult.lies(&k("master.transitions")).unwrap(),
            Wert::Text("weich/8".into())
        );
    }

    /// Ein Schnitt und eine Blende sind nicht dasselbe — und dreimal derselbe
    /// Befehl ist ein Übergang, nicht drei.
    #[test]
    fn verschiedene_griffe_sind_keine_wiederholung() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        aufgelegt(&mut pult);

        // Ein Sprung hinüber, dreimal geschickt.
        for _ in 0..3 {
            pult.schreibe(&k("master.crossfader"), Wert::Zahl(1.0))
                .unwrap();
        }
        assert_eq!(pult.lies(&k("master.repeats")).unwrap(), Wert::Zahl(1.0));

        // Und wieder zurück, diesmal als Blende.
        rampe_planen(
            &pult,
            &mut plan,
            k("master.crossfader"),
            -1.0,
            8.0,
            None,
            Form::Linear,
        )
        .expect("planen");
        laufen(
            &mut pult,
            &mut plan,
            &mut runner,
            (RATE as f64 * 4.2) as usize,
        );

        assert_eq!(
            pult.lies(&k("master.transitions")).unwrap(),
            Wert::Text("schnitt, linear/8".into())
        );
        assert_eq!(pult.lies(&k("master.repeats")).unwrap(), Wert::Zahl(1.0));
    }

    /// **Ein Deck, das zurückspringt, bricht den Plan ab — statt ihn zu
    /// wiederholen.**
    ///
    /// Gefunden am laufenden Programm beim ersten Schleifen-Übergang: Der
    /// Crossfader fuhr sieben Mal hin und her. Eine Rampe rechnet ihren
    /// Fortschritt aus dem Beat des Taktgebers; läuft der in einer Schleife,
    /// fängt sie bei jedem Durchlauf von vorn an. Ein Vorgemerktes hinter dem
    /// Schleifenende kommt umgekehrt nie an.
    ///
    /// Beides still zu tun wäre das Schlimmste: Der Bediener sieht einen Plan,
    /// der läuft, und hört etwas anderes.
    #[test]
    fn ein_zurueckgesprungenes_deck_bricht_den_plan_ab_statt_ihn_zu_wiederholen() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        rampe_planen(
            &pult,
            &mut plan,
            k("master.crossfader"),
            1.0,
            32.0,
            None,
            Form::Linear,
        )
        .expect("planen");

        // Ein Stück laufen lassen, dann zurückspringen — genau das tut eine
        // Schleife an ihrem Ende.
        laufen(
            &mut pult,
            &mut plan,
            &mut runner,
            (RATE as f64 * 2.0) as usize,
        );
        assert!(!plan.ist_leer(), "die Rampe ist zu früh fertig");
        pult.decks()[0].state.seek_frames(0);

        let meldungen = laufen(&mut pult, &mut plan, &mut runner, RATE as usize / 4);
        assert!(plan.ist_leer(), "der Auftrag läuft nach dem Sprung weiter");
        assert!(
            meldungen.iter().any(|m| m.contains("zurückgesprungen")),
            "kein Wort vom Rücksprung: {meldungen:?}"
        );
    }

    /// `in deck2 phrase …` — der Taktgeber darf ein anderer sein als das Deck,
    /// auf das gewirkt wird. Ohne das ließe sich ein schleifendes Deck nicht
    /// wieder auflösen: Sein eigener Beat kommt nie über das Schleifenende.
    #[test]
    fn in_nimmt_ein_eigenes_taktgeber_deck() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        pult.schreibe(&k("channel2.fader"), Wert::Zahl(1.0))
            .unwrap();
        pult.schreibe(&k("deck2.play"), Wert::Schalter(true))
            .unwrap();
        rendern(&mut runner, 256);

        let mut sitzung = crate::Sitzung::neu();
        let antwort = crate::behandle(
            &mut pult,
            &mut sitzung,
            "in deck2 4 set deck1.loop_active 0",
        );
        assert!(antwort.starts_with("ok plan "), "{antwort}");
        assert_eq!(
            pult.plan.auftraege()[0].takt_deck,
            1,
            "der Auftrag hängt am falschen Deck"
        );
    }

    /// Der Mensch gewinnt.
    ///
    /// Ohne das wäre eine laufende Rampe stärker als der Griff daneben: Man
    /// zieht den Fader zu, und einen Wimpernschlag später steht er wieder
    /// offen. Für ein Team von Agenten gilt dasselbe untereinander.
    #[test]
    fn ein_fremder_griff_loest_die_rampe_ab() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            32.0,
            None,
            Form::Linear,
        )
        .expect("planen");
        laufen(&mut pult, &mut plan, &mut runner, RATE as usize);
        assert!(!plan.ist_leer(), "die Rampe sollte noch laufen");

        // Jemand anders greift ein.
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(0.2))
            .unwrap();
        let meldungen = laufen(&mut pult, &mut plan, &mut runner, RATE as usize / 2);

        assert!(plan.ist_leer(), "die Rampe läuft über den Eingriff hinweg");
        assert!(
            meldungen.iter().any(|m| m.contains("abgeloest")),
            "{meldungen:?}"
        );
        // Und der fremde Wert steht noch da, wo er hingesetzt wurde.
        let Wert::Zahl(steht) = pult.lies(&k("channel1.fader")).unwrap() else {
            panic!()
        };
        assert!((steht - 0.2).abs() < 1e-3, "Fader steht bei {steht:.3}");
    }

    /// Ein stehendes Deck hat keine Takte — der Plan wartet, statt zu laufen.
    #[test]
    fn ein_gestopptes_deck_haelt_den_plan_an() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();
        // deck1 bleibt stehen.

        rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            4.0,
            None,
            Form::Linear,
        )
        .expect("planen");
        laufen(&mut pult, &mut plan, &mut runner, RATE as usize * 3);

        assert!(!plan.ist_leer(), "die Rampe lief ohne laufendes Deck");
        assert_eq!(pult.lies(&k("channel1.fader")).unwrap(), Wert::Zahl(1.0));
    }

    /// Ein Befehl, der auf seinen Beat wartet.
    #[test]
    fn ein_vorgemerkter_befehl_kommt_zu_seiner_zeit() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        spaeter_planen(&pult, &mut plan, 4.0, "set channel2.fader 0.9".into(), None)
            .expect("planen");

        let mut gelaufen = Vec::new();
        for _ in 0..40 {
            rendern(&mut runner, RATE as usize / 20);
            takt(&mut pult, &mut plan, &mut |_, zeile| {
                gelaufen.push(zeile.to_string());
                "ok".into()
            });
        }

        assert_eq!(gelaufen, vec!["set channel2.fader 0.9"]);
        assert!(plan.ist_leer());
    }

    /// Der Taktgeber kommt vom Kanal, ohne dass man ihn nennen muss.
    #[test]
    fn ein_kanal_erbt_den_takt_von_seinem_deck() {
        let (pult, _runner) = pult_mit_zwei_decks();

        assert_eq!(taktgeber(&pult, &k("channel2.fader")), Some(1));
        assert_eq!(taktgeber(&pult, &k("deck2.tempo")), Some(1));
        // Der AUX-Kanal hat kein Deck.
        assert_eq!(taktgeber(&pult, &k("channel3.fader")), None);
        // Die Summe nimmt das erste Deck mit Grid.
        assert_eq!(taktgeber(&pult, &k("master.crossfader")), Some(0));
    }

    #[test]
    fn was_sich_nicht_rampen_laesst_wird_abgewiesen() {
        let (pult, _runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();

        // Ein Schalter ist keine Bewegung.
        assert!(rampe_planen(
            &pult,
            &mut plan,
            k("deck1.play"),
            1.0,
            8.0,
            None,
            Form::Linear
        )
        .is_err());
        // Negative Längen ergeben keinen Sinn.
        assert!(rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            -4.0,
            None,
            Form::Linear
        )
        .is_err());
        assert!(plan.ist_leer());
    }

    /// Ein fällig gewordener Befehl darf keine Nummer wiederverwenden.
    ///
    /// Der Taktgeber nimmt den Plan aus dem Pult heraus, arbeitet ihn ab und
    /// legt ihn zurück. Was währenddessen neu vorgemerkt wird — ein
    /// `in 0 ramp …`, das gerade fällig wurde —, landet im leeren Plan, der
    /// solange im Pult liegt. Ohne übernommenen Zählerstand bekäme es dort
    /// wieder die Nummer 1, und `cancel 1` träfe dann zwei Aufträge.
    #[test]
    fn ein_nachgelegter_auftrag_bekommt_keine_schon_vergebene_nummer() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        // Ein langer Läufer, der die 1 belegt und belegt bleibt.
        rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            512.0,
            None,
            Form::Linear,
        )
        .unwrap();
        // Und ein Befehl, der sofort fällig ist und selbst etwas vormerkt.
        spaeter_planen(
            &pult,
            &mut plan,
            0.0,
            "ramp channel2.fader 0 64".into(),
            None,
        )
        .unwrap();

        rendern(&mut runner, RATE as usize / 10);
        takt(&mut pult, &mut plan, &mut |pult, zeile| {
            let mut sitzung = crate::Sitzung::neu();
            crate::behandle(pult, &mut sitzung, zeile)
        });

        // Der Taktgeber legt zurück, was währenddessen dazukam — wie im Betrieb.
        let neu = std::mem::take(&mut pult.plan);
        for a in neu.auftraege() {
            plan.uebernehmen(a.clone());
        }

        let nummern: Vec<u64> = plan.auftraege().iter().map(|a| a.id).collect();
        let mut sortiert = nummern.clone();
        sortiert.sort_unstable();
        sortiert.dedup();
        assert_eq!(
            sortiert.len(),
            nummern.len(),
            "zwei Aufträge tragen dieselbe Nummer: {nummern:?}"
        );
    }

    #[test]
    fn vorgemerktes_laesst_sich_zuruecknehmen() {
        let (pult, _runner) = pult_mit_zwei_decks();
        let mut plan = Zeitplan::neu();

        let a = rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            8.0,
            None,
            Form::Linear,
        )
        .unwrap();
        rampe_planen(
            &pult,
            &mut plan,
            k("channel2.fader"),
            0.0,
            8.0,
            None,
            Form::Linear,
        )
        .unwrap();

        assert_eq!(plan.streichen(Some(a)), 1);
        assert_eq!(plan.auftraege().len(), 1);
        assert_eq!(plan.streichen(None), 1);
        assert!(plan.ist_leer());
    }

    /// Was der Plan meldet, gehört in die Mitschrift — und zwar mit dem Frame.
    ///
    /// Das Ende einer langen Blende ist im Klang genauso schwer zu finden wie
    /// ihr Anfang: Dort läuft der eingehende Track längst allein. Der Plan
    /// weiß es auf den Takt.
    #[test]
    fn was_der_plan_meldet_steht_in_der_mitschrift() {
        let (mut pult, mut runner) = pult_mit_zwei_decks();
        let wav = std::env::temp_dir().join(format!("musik-plan-{}.wav", std::process::id()));
        let neben = wav.with_extension(crate::mitschrift::ENDUNG);
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&neben);

        pult.ausloesen(&k("master.record"), Some(&wav.to_string_lossy()))
            .expect("Mitschnitt");
        pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
            .unwrap();
        pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
            .unwrap();

        let mut plan = Zeitplan::neu();
        // Acht Beats bei 128 BPM sind 3,75 s.
        rampe_planen(
            &pult,
            &mut plan,
            k("channel1.fader"),
            0.0,
            8.0,
            None,
            Form::Linear,
        )
        .expect("planen");
        laufen(&mut pult, &mut plan, &mut runner, RATE as usize * 4);

        let p = crate::mitschrift::lesen(&neben).expect("lesbar");
        let fertig = p
            .ereignisse
            .iter()
            .find(|e| e.text.contains("fertig"))
            .expect("das Ende der Blende steht nicht in der Mitschrift");
        let sek = fertig.sekunden(p.kopf.rate);
        assert!(
            (3.5..4.1).contains(&sek),
            "die Blende endete laut Mitschrift bei {sek:.2} s, erwartet wurden rund 3,75"
        );
        assert!(
            fertig.stand(0).is_some_and(|s| s.beat > 7.5),
            "der Beat beim Ende fehlt oder passt nicht: {:?}",
            fertig.staende
        );

        pult.ausloesen(&k("master.record_stop"), None)
            .expect("Stop");
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&neben);
    }
}

/// Die Formen: dieselbe Strecke, andere Verteilung.
#[cfg(test)]
mod form_tests {
    use super::*;
    use crate::testing::{pult_mit_zwei_decks, rendern, RATE};
    use crate::wert::Wert;

    fn k(text: &str) -> Schluessel {
        Schluessel::parse(text).unwrap()
    }

    /// **Jede Form muss 0 auf 0 und 1 auf 1 abbilden.** Sonst spränge der
    /// Regler beim Losfahren oder käme am Ende nicht an — und das wäre kein
    /// Geschmack, sondern ein Fehler.
    #[test]
    fn jede_form_faengt_bei_null_an_und_kommt_bei_eins_an() {
        for f in Form::ALLE {
            assert_eq!(f.anwenden(0.0), 0.0, "{}", f.name());
            assert_eq!(f.anwenden(1.0), 1.0, "{}", f.name());
            // Und außerhalb wird begrenzt, statt über das Ziel hinauszulaufen.
            assert_eq!(f.anwenden(-1.0), 0.0, "{}", f.name());
            assert_eq!(f.anwenden(2.0), 1.0, "{}", f.name());
        }
    }

    /// Keine Form darf rückwärtslaufen: Ein Fader, der zwischendurch umkehrt,
    /// ist kaputt und nicht ausdrucksstark.
    #[test]
    fn keine_form_laeuft_rueckwaerts() {
        for f in Form::ALLE {
            let mut vorher = 0.0;
            for i in 0..=100 {
                let jetzt = f.anwenden(i as f64 / 100.0);
                assert!(
                    jetzt >= vorher - 1e-9,
                    "{} läuft bei {i} % zurück: {vorher} → {jetzt}",
                    f.name()
                );
                vorher = jetzt;
            }
        }
    }

    /// Der Unterschied, um den es geht: Auf halber Strecke stehen die Formen
    /// woanders. `spaet` hat noch kaum etwas getan, `frueh` schon fast alles.
    #[test]
    fn auf_halber_strecke_stehen_sie_woanders() {
        assert_eq!(Form::Linear.anwenden(0.5), 0.5);
        assert_eq!(
            Form::Weich.anwenden(0.5),
            0.5,
            "die S-Kurve ist symmetrisch"
        );
        assert_eq!(Form::Spaet.anwenden(0.5), 0.25);
        assert_eq!(Form::Frueh.anwenden(0.5), 0.75);

        // Nach einem Viertel: der Unterschied ist am größten, wo er zählt.
        assert!(Form::Spaet.anwenden(0.25) < 0.1, "spaet zieht zu früh los");
        assert!(Form::Frueh.anwenden(0.25) > 0.4, "frueh zögert");
    }

    /// Und das Ganze am laufenden Regler, nicht nur in der Rechnung.
    #[test]
    fn eine_weiche_rampe_haengt_in_der_mitte_der_linearen_nach_oder_vor() {
        for (form, erwartet) in [
            (Form::Spaet, 0.25f64),
            (Form::Linear, 0.5),
            (Form::Frueh, 0.75),
        ] {
            let (mut pult, mut runner) = pult_mit_zwei_decks();
            let mut plan = Zeitplan::neu();
            pult.schreibe(&k("channel1.fader"), Wert::Zahl(1.0))
                .unwrap();
            pult.schreibe(&k("deck1.play"), Wert::Schalter(true))
                .unwrap();
            rampe_planen(&pult, &mut plan, k("channel1.fader"), 0.0, 8.0, None, form)
                .expect("planen");

            // Acht Beats bei 128 BPM sind 3,75 s — die Hälfte davon.
            for _ in 0..20 {
                rendern(&mut runner, (RATE as f64 * 1.875 / 20.0) as usize);
                takt(&mut pult, &mut plan, &mut |_, _| String::new());
            }

            let Wert::Zahl(steht) = pult.lies(&k("channel1.fader")).unwrap() else {
                panic!()
            };
            // Der Fader läuft von 1 nach 0, der Anteil also andersherum.
            let gefahren = 1.0 - steht;
            assert!(
                (gefahren - erwartet).abs() < 0.12,
                "{}: nach der Hälfte sind {gefahren:.2} gefahren, erwartet {erwartet}",
                form.name()
            );
        }
    }

    #[test]
    fn jede_form_ueberlebt_ihren_namen() {
        for f in Form::ALLE {
            assert_eq!(Form::parse(f.name()), Some(f));
        }
        assert_eq!(Form::parse("sanft"), None);
        assert_eq!(Form::default(), Form::Linear);
    }
}
