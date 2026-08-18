//! Was es gibt — und was es bedeutet.
//!
//! Das ist der Teil, den Mixxx nicht hat. Dort ist die Liste der Controls
//! *Dokumentation*: Sie steht im Handbuch, und wer von außen steuern will,
//! liest sie und tippt die Namen ab. Läuft das Programm, lässt sich nicht
//! erfragen, was es kann.
//!
//! Hier trägt jedes Control seinen Bereich, seine Einheit, seinen Standardwert
//! und einen Satz zur Bedeutung mit sich. Ein Agent, ein Controller-Mapping
//! oder ein Skript kann das Pult zur Laufzeit **fragen**, statt es zu wissen.
//! Genau darauf setzt später die MCP-Schicht auf: Werkzeugbeschreibungen lassen
//! sich daraus erzeugen, statt sie von Hand doppelt zu pflegen.
//!
//! Die Namen sind englisch, obwohl der Rest des Codes deutsch kommentiert ist.
//! Sie sind die Schnittstelle nach außen, und die spricht die Sprache, die
//! Controller-Mappings und Agenten ohnehin verwenden.

use crate::wert::{Art, Einheit};

#[derive(Debug, Clone)]
pub struct Beschreibung {
    pub element: &'static str,
    pub art: Art,
    /// Nur bei [`Art::Zahl`]: kleinster und größter sinnvoller Wert.
    pub bereich: Option<(f64, f64)>,
    pub einheit: Einheit,
    pub auswahl: &'static [&'static str],
    /// Kurzbeschriftung für enge Stellen — ein Kanalzug ist keine
    /// Handbuchseite. Leer heißt: keine sinnvolle Kurzform, das Control
    /// gehört nicht auf einen Knopf.
    pub kurz: &'static str,
    pub schreibbar: bool,
    /// Nur bei [`Art::Aktion`]: was der Auslöser als Argument erwartet.
    pub argument: &'static str,
    pub text: &'static str,
}

impl Beschreibung {
    /// Wert innerhalb des Bereichs zurechtstutzen.
    ///
    /// Ein Fader über 1.0 ist kein Fehler, den man dem Aufrufer um die Ohren
    /// hauen muss — ein MIDI-Regler, der 127 sendet, meint das Maximum. Werte
    /// außerhalb werden deshalb begrenzt, nicht abgelehnt.
    pub fn begrenzen(&self, wert: f64) -> f64 {
        match self.bereich {
            Some((min, max)) => wert.clamp(min, max),
            None => wert,
        }
    }

    /// 0..1 in den echten Bereich. Für Controller, die nur Prozent kennen.
    pub fn aus_normiert(&self, norm: f64) -> f64 {
        match self.bereich {
            Some((min, max)) => min + norm.clamp(0.0, 1.0) * (max - min),
            None => norm,
        }
    }

    /// Echter Bereich nach 0..1.
    pub fn nach_normiert(&self, wert: f64) -> f64 {
        match self.bereich {
            Some((min, max)) if max > min => ((wert - min) / (max - min)).clamp(0.0, 1.0),
            _ => wert,
        }
    }
}

#[allow(clippy::too_many_arguments)]
const fn zahl(
    element: &'static str,
    kurz: &'static str,
    min: f64,
    max: f64,
    einheit: Einheit,
    schreibbar: bool,
    text: &'static str,
) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Zahl,
        bereich: Some((min, max)),
        einheit,
        auswahl: &[],
        kurz,
        schreibbar,
        argument: "",
        text,
    }
}

const fn schalter(
    element: &'static str,
    kurz: &'static str,
    schreibbar: bool,
    text: &'static str,
) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Schalter,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &[],
        kurz,
        schreibbar,
        argument: "",
        text,
    }
}

/// Ein Auslöser. `argument` beschreibt, was er erwartet — leer heißt: nichts.
const fn aktion(element: &'static str, argument: &'static str, text: &'static str) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Aktion,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &[],
        kurz: "",
        schreibbar: true,
        argument,
        text,
    }
}

/// Ein Textfeld, das sich auch beschreiben lässt.
const fn text_schreibbar(element: &'static str, text: &'static str) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Text,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &[],
        kurz: "",
        schreibbar: true,
        argument: "",
        text,
    }
}

const fn text_feld(element: &'static str, text: &'static str) -> Beschreibung {
    Beschreibung {
        element,
        art: Art::Text,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &[],
        kurz: "",
        schreibbar: false,
        argument: "",
        text,
    }
}

/// Obergrenze des Tempo-Reglers. ±8 % ist der Regelweg eines Technics-Decks
/// und damit das, was DJs im Griff haben.
pub const TEMPO_MIN: f64 = 0.92;
pub const TEMPO_MAX: f64 = 1.08;

/// Hot Cues, die es je Deck gibt. Muss zu `audio_core::deck::HOT_CUES` passen.
pub const HOT_CUES: usize = audio_core::deck::HOT_CUES;

pub static DECK: &[Beschreibung] = &[
    schalter("play", "PLAY", true, "Läuft das Deck"),
    zahl(
        "position",
        "POS",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        true,
        "Abspielposition; Schreiben springt",
    ),
    zahl(
        "duration",
        "LEN",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Länge des geladenen Tracks",
    ),
    zahl(
        "bpm",
        "BPM",
        0.0,
        400.0,
        Einheit::Bpm,
        false,
        "Tempo einschließlich Tempo-Regler",
    ),
    zahl(
        "bpm_grid",
        "GRID",
        0.0,
        400.0,
        Einheit::Bpm,
        true,
        "Tempo des Beatgrids, ohne Regler; schreiben korrigiert es und merkt es sich",
    ),
    zahl(
        "grid_anchor",
        "ANKER",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        true,
        "Wo der erste Beat liegt; schreiben verschiebt das ganze Raster",
    ),
    zahl(
        "tempo",
        "TEMPO",
        TEMPO_MIN,
        TEMPO_MAX,
        Einheit::Faktor,
        true,
        "Tempo-Regler; 1.0 ist die Originalgeschwindigkeit",
    ),
    schalter(
        "keylock",
        "KEYLOCK",
        true,
        "Tonhöhe beim Tempowechsel halten",
    ),
    zahl(
        "beat_phase",
        "PHASE",
        0.0,
        1.0,
        Einheit::Beats,
        false,
        "Lage im Beat, 0 ist auf dem Schlag",
    ),
    // Was ein Bediener wirklich wissen will, statt der Zahlen, aus denen er es
    // sich sonst selbst ausrechnet. Position, Länge und Tempo stehen daneben —
    // aber wer daraus „noch 32 Beats" ableiten muss, rechnet es bei jedem
    // Blick neu und macht dabei irgendwann einen Fehler, den niemand sieht.
    zahl(
        "beat",
        "BEAT",
        0.0,
        f64::MAX,
        Einheit::Beats,
        false,
        "Der wievielte Beat gerade läuft, vom Grid-Anker gezählt",
    ),
    zahl(
        "beats_left",
        "REST",
        0.0,
        f64::MAX,
        Einheit::Beats,
        false,
        "Beats bis zum Ende des Tracks — danach richtet sich, wann der Übergang anfängt",
    ),
    zahl(
        "phrase_beats",
        "PHRASE",
        1.0,
        64.0,
        Einheit::Beats,
        true,
        "Wie lang eine Phrase ist; 16 passt zu den meisten Tanzstücken",
    ),
    zahl(
        "beats_to_phrase",
        "BIS PHRASE",
        0.0,
        64.0,
        Einheit::Beats,
        false,
        "Beats bis zur nächsten Phrasengrenze — dort setzt man ein, nicht irgendwo",
    ),
    schalter("loop_active", "LOOP", true, "Läuft gerade eine Schleife"),
    zahl(
        "loop_beats",
        "LOOP",
        0.0,
        64.0,
        Einheit::Beats,
        true,
        "Schleife dieser Länge ab der Position setzen",
    ),
    // Die Gliederung (S2). Ohne sie war „blende aus, während das Outro läuft"
    // nicht sagbar — und der eingehende Track setzte auf Sekunde 0 ein statt
    // auf einem Downbeat.
    text_feld(
        "section",
        "Welcher Abschnitt gerade läuft: intro, aufbau, drop, break, outro oder teil",
    ),
    zahl(
        "section_beats_left",
        "ABSCHNITT",
        0.0,
        f64::MAX,
        Einheit::Beats,
        false,
        "Beats bis zum Ende des laufenden Abschnitts",
    ),
    zahl(
        "beats_to_outro",
        "BIS OUTRO",
        f64::MIN,
        f64::MAX,
        Einheit::Beats,
        false,
        "Beats bis zum Anfang des Outros; negativ, sobald es läuft — die Stelle, \
an der ein Übergang sitzen darf",
    ),
    zahl(
        "intro_beats",
        "INTRO",
        0.0,
        f64::MAX,
        Einheit::Beats,
        false,
        "Wie lang das Intro ist — danach sucht man einen Track zum Einmischen aus",
    ),
    zahl(
        "entry",
        "EINSTIEG",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Wo der Track einsetzen sollte: sein erster Downbeat, nicht Sekunde 0",
    ),
    text_feld("title", "Titel des geladenen Tracks"),
    text_feld("artist", "Künstler des geladenen Tracks"),
    text_feld(
        "key",
        "Tonart des geladenen Tracks, etwa Am oder F#; leer heißt unbekannt",
    ),
    text_feld(
        "key_camelot",
        "Dieselbe Tonart als Camelot-Zahl (8A, 5B) — danach wird harmonisch gemischt",
    ),
    // Ohne das erfährt ein Agent nie, dass er den nächsten Track auflegen
    // muss. Für autonomes Auflegen ist es die zentrale Information.
    schalter("finished", "", false, "Track ist durchgelaufen"),
    text_feld(
        "load_status",
        "Stand des letzten Ladeauftrags: bereit, laedt oder ein Fehler",
    ),
    aktion(
        "sync",
        "[deck]",
        "Auf das andere Deck ziehen — Tempo UND Phase; ohne Argument das jeweils andere",
    ),
    aktion(
        "load",
        "<pfad>",
        "Track laden; arbeitet im Hintergrund, Fortschritt über load_status",
    ),
    aktion("jump_cue", "<1..8>", "Einen gesetzten Hot Cue anspringen"),
    aktion(
        "jump_entry",
        "",
        "Auf den Einstiegspunkt springen — den ersten Downbeat statt Sekunde 0",
    ),
    aktion(
        "beatjump",
        "<beats>",
        "Um so viele Beats springen, negativ zurück",
    ),
    aktion(
        "grid_here",
        "",
        "Den Anker auf die aktuelle Position legen — der Beat ist hier",
    ),
    aktion(
        "grid_scale",
        "<faktor>",
        "Grid-Tempo mit diesem Faktor multiplizieren; 0.5 und 2 räumen Oktavfehler auf",
    ),
];

/// Hot Cues heißen `cue1` bis `cue8` und lassen sich nicht als feste Liste
/// hinschreiben, ohne sie achtmal zu wiederholen.
pub fn hot_cue_beschreibung(index: usize) -> Option<Beschreibung> {
    if index >= HOT_CUES {
        return None;
    }

    Some(Beschreibung {
        // Der Name wird zur Laufzeit gebraucht, `element` ist aber `'static`.
        // Deshalb eine kleine feste Tabelle statt einer geleakten Zeichenkette.
        element: HOT_CUE_NAMEN[index],
        kurz: "",
        art: Art::Zahl,
        bereich: Some((0.0, f64::MAX)),
        einheit: Einheit::Sekunden,
        auswahl: &[],
        schreibbar: true,
        argument: "",
        text: "Hot Cue; Schreiben setzt ihn, Lesen gibt seine Position oder '-'",
    })
}

static HOT_CUE_NAMEN: [&str; HOT_CUES] = [
    "cue1", "cue2", "cue3", "cue4", "cue5", "cue6", "cue7", "cue8",
];

/// Ein Kanalzug, in der Reihenfolge, in der er am Gerät steht.
///
/// Die Reihenfolge ist nicht beliebig: Die Oberfläche holt sich ihre Regler
/// aus dem Katalog und zeichnet sie so, wie sie hier stehen. Höhen oben,
/// Bässe unten — andersherum greift man beim Auflegen ständig daneben.
pub static KANAL: &[Beschreibung] = &[
    zahl(
        "trim",
        "TRIM",
        0.0,
        2.0,
        Einheit::Faktor,
        true,
        "Eingangsverstärkung vor dem EQ",
    ),
    zahl("eq_high", "HI", 0.0, 2.0, Einheit::Faktor, true, "Höhen"),
    zahl("eq_mid", "MID", 0.0, 2.0, Einheit::Faktor, true, "Mitten"),
    zahl(
        "eq_low",
        "LOW",
        0.0,
        2.0,
        Einheit::Faktor,
        true,
        "Bässe; 0 ist ein echter Kill",
    ),
    zahl(
        "filter",
        "FLT",
        -1.0,
        1.0,
        Einheit::Bipolar,
        true,
        "DJ-Filter; negativ Tiefpass, positiv Hochpass",
    ),
    zahl(
        "fader",
        "FADER",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Linefader",
    ),
    schalter("cue", "CUE", true, "Kanal auf den Kopfhörer legen"),
    Beschreibung {
        element: "fx",
        kurz: "FX",
        art: Art::Auswahl,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: audio_engine::Effekt::NAMEN,
        schreibbar: true,
        argument: "",
        text: "Effekt hinter dem Fader; die Fahne überlebt den zugezogenen Fader",
    },
    zahl(
        "fx_mix",
        "MIX",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Trocken bis nass",
    ),
    zahl(
        "fx_amount",
        "AMT",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Stärke: Rückkopplung, Öffnungsdauer, Tiefe oder Härte",
    ),
    zahl(
        "fx_time",
        "ZEIT",
        0.001,
        4.0,
        Einheit::Sekunden,
        true,
        "Effektzeit; mit fx_sync auf das Tempo des Decks setzen",
    ),
    aktion(
        "fx_sync",
        "<beats>",
        "Effektzeit auf so viele Beats des zugehörigen Decks setzen",
    ),
    Beschreibung {
        element: "assign",
        kurz: "",
        art: Art::Auswahl,
        bereich: None,
        einheit: Einheit::Keine,
        auswahl: &["a", "b", "thru"],
        schreibbar: true,
        argument: "",
        text: "Seite am Crossfader; 'thru' geht am Crossfader vorbei",
    },
];

pub static MASTER: &[Beschreibung] = &[
    aktion(
        "record",
        "<pfad>",
        "Mitschnitt der Summe starten; nimmt auf, was auf die Anlage geht",
    ),
    aktion(
        "record_stop",
        "",
        "Mitschnitt beenden und die Datei abschließen",
    ),
    // Der einzige Weg, auf dem ein Bediener erfährt, dass ihm jemand anders
    // dazwischengekommen ist. Ein Abo darauf meldet **jede** Zeile, nicht nur
    // die zuletzt geschriebene — siehe `Sitzung::aenderungen`.
    text_feld(
        "events",
        "Was der Plan zuletzt gemeldet hat: fertig, abgeloest, abgebrochen, gestrichen. \
Abonnieren meldet jede Zeile einzeln, mit 'event' davor",
    ),
    // Der Bogen: was das Set vorhat, bevor der nächste Track gewählt wird.
    text_schreibbar(
        "arc",
        "Ziel-Energiekurve über die Setdauer: '0 0.3, 20 0.7, 45 0.95, 60 0.5'. \
Zeiten in Minuten, Energie 0 bis 1. Schreibbar",
    ),
    zahl(
        "arc_minutes",
        "",
        0.0,
        f64::MAX,
        Einheit::Keine,
        false,
        "Wie lange das Set schon läuft, in Minuten; leer vor arc_start",
    ),
    zahl(
        "arc_target",
        "SOLL",
        0.0,
        1.0,
        Einheit::Keine,
        false,
        "Wie viel Energie der Bogen hier vorsieht",
    ),
    zahl(
        "arc_actual",
        "IST",
        0.0,
        1.0,
        Einheit::Keine,
        false,
        "Wie viel gerade läuft — aus der Art des Abschnitts, nicht aus dem Pegel: \
der ist auf den Track selbst bezogen und über Tracks hinweg nicht vergleichbar",
    ),
    zahl(
        "arc_gap",
        "LÜCKE",
        -1.0,
        1.0,
        Einheit::Keine,
        false,
        "Soll minus Ist. Positiv heißt: der Bogen will mehr, als gerade läuft — \
die Zahl, nach der der nächste Track gewählt wird",
    ),
    text_feld(
        "arc_trend",
        "Wohin der Bogen als Nächstes will: steigt, haelt oder faellt",
    ),
    aktion(
        "arc_start",
        "",
        "Das Set beginnt jetzt — ohne das gibt es keinen Ort auf dem Bogen",
    ),
    aktion(
        "uebergang",
        "<blende|bassswap|schnitt|filter> [beats]",
        "Einen benannten Übergang vormerken — die Zeilen dafür stehen in der Antwort \
und im Plan. Die Anlage wählt nicht aus: welcher Griff passt, entscheidet, wer \
begründen kann warum",
    ),
    zahl(
        "event_count",
        "",
        0.0,
        f64::MAX,
        Einheit::Keine,
        false,
        "Wie viele Ereignisse es insgesamt gab. Springt die Zahl um mehr als eins, \
hat man dazwischen welche verpasst — für alle, die fragen statt zu abonnieren",
    ),
    schalter("recording", "", false, "Läuft gerade ein Mitschnitt"),
    zahl(
        "record_seconds",
        "",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Bisherige Länge des Mitschnitts",
    ),
    zahl(
        "record_dropped",
        "",
        0.0,
        f64::MAX,
        Einheit::Keine,
        false,
        "Frames, die der Schreiber nicht mehr annehmen konnte — alles über 0 heißt Lücken",
    ),
    aktion(
        "search",
        "<text>",
        "Die Sammlung durchsuchen; antwortet mit einer Zeile je Treffer",
    ),
    aktion("playlists", "", "Die Playlists der Sammlung aufzählen"),
    aktion(
        "playlist",
        "<name>",
        "Die Tracks einer Playlist auflisten, in ihrer Reihenfolge",
    ),
    aktion(
        "search_mixable",
        "<bpm>",
        "Tracks suchen, die tempomäßig zu diesem Wert passen",
    ),
    aktion(
        "search_harmonic",
        "<tonart>",
        "Tracks suchen, deren Tonart harmonisch passt; nimmt Am, F# oder 8A",
    ),
    // Was von außen hereinkommt: die Reaktion des Raums. Feste Plätze mit
    // beschriftbarem Namen — wie ein Kanalzug, den man mit Klebeband
    // beschriftet. Dynamische Namen bräuchten geleakte Zeichenketten, und
    // dieselbe Entscheidung ist schon bei den Hot Cues gefallen.
    zahl(
        "signal1",
        "SIG1",
        -1.0,
        1.0,
        Einheit::Keine,
        true,
        "Signal 1 von außen; -1 bis 1, wofür es steht sagt signal1_name",
    ),
    text_schreibbar(
        "signal1_name",
        "Wofür Signal 1 steht, etwa 'Energie auf der Flaeche'; leer heisst ungenutzt",
    ),
    zahl(
        "signal1_trend",
        "TRD1",
        -100.0,
        100.0,
        Einheit::Keine,
        false,
        "Änderung von Signal 1 je Minute; positiv steigt, leer heisst zu wenige Proben",
    ),
    zahl(
        "signal1_age",
        "ALT1",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Sekunden seit der letzten Meldung an Signal 1; alte Werte lügen nicht, sie sind nur alt",
    ),
    zahl(
        "signal2",
        "SIG2",
        -1.0,
        1.0,
        Einheit::Keine,
        true,
        "Signal 2 von außen; -1 bis 1, wofür es steht sagt signal2_name",
    ),
    text_schreibbar(
        "signal2_name",
        "Wofür Signal 2 steht, etwa 'Energie auf der Flaeche'; leer heisst ungenutzt",
    ),
    zahl(
        "signal2_trend",
        "TRD2",
        -100.0,
        100.0,
        Einheit::Keine,
        false,
        "Änderung von Signal 2 je Minute; positiv steigt, leer heisst zu wenige Proben",
    ),
    zahl(
        "signal2_age",
        "ALT2",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Sekunden seit der letzten Meldung an Signal 2; alte Werte lügen nicht, sie sind nur alt",
    ),
    zahl(
        "signal3",
        "SIG3",
        -1.0,
        1.0,
        Einheit::Keine,
        true,
        "Signal 3 von außen; -1 bis 1, wofür es steht sagt signal3_name",
    ),
    text_schreibbar(
        "signal3_name",
        "Wofür Signal 3 steht, etwa 'Energie auf der Flaeche'; leer heisst ungenutzt",
    ),
    zahl(
        "signal3_trend",
        "TRD3",
        -100.0,
        100.0,
        Einheit::Keine,
        false,
        "Änderung von Signal 3 je Minute; positiv steigt, leer heisst zu wenige Proben",
    ),
    zahl(
        "signal3_age",
        "ALT3",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Sekunden seit der letzten Meldung an Signal 3; alte Werte lügen nicht, sie sind nur alt",
    ),
    zahl(
        "signal4",
        "SIG4",
        -1.0,
        1.0,
        Einheit::Keine,
        true,
        "Signal 4 von außen; -1 bis 1, wofür es steht sagt signal4_name",
    ),
    text_schreibbar(
        "signal4_name",
        "Wofür Signal 4 steht, etwa 'Energie auf der Flaeche'; leer heisst ungenutzt",
    ),
    zahl(
        "signal4_trend",
        "TRD4",
        -100.0,
        100.0,
        Einheit::Keine,
        false,
        "Änderung von Signal 4 je Minute; positiv steigt, leer heisst zu wenige Proben",
    ),
    zahl(
        "signal4_age",
        "ALT4",
        0.0,
        f64::MAX,
        Einheit::Sekunden,
        false,
        "Sekunden seit der letzten Meldung an Signal 4; alte Werte lügen nicht, sie sind nur alt",
    ),
    aktion(
        "queue",
        "",
        "Was als Nächstes kommt — eine Zeile je Eintrag, mit Nummer und Notiz",
    ),
    aktion(
        "queue_add",
        "<pfad>",
        "Einen Track hinten anhängen; derselbe Pfad wird nicht zweimal angenommen",
    ),
    aktion(
        "queue_note",
        "<nr> <text>",
        "Notieren, warum ein Eintrag dort steht — auch, um es zu widerrufen",
    ),
    aktion(
        "queue_bump",
        "<nr>",
        "Einen Eintrag nach vorn ziehen, also zum Nächsten machen",
    ),
    aktion("queue_drop", "<nr>", "Einen Eintrag herausnehmen"),
    aktion(
        "queue_clear",
        "",
        "Die ganze Liste leeren — auch, was andere vorgemerkt haben",
    ),
    aktion(
        "queue_next",
        "[deck]",
        "Den vordersten Eintrag laden; ohne Angabe auf ein Deck, das nicht läuft",
    ),
    zahl(
        "crossfader",
        "XFADER",
        -1.0,
        1.0,
        Einheit::Bipolar,
        true,
        "Crossfader; -1 ist ganz A, +1 ganz B",
    ),
    zahl(
        "crossfader_curve",
        "KURVE",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Kurve; 0 weich, 1 hart",
    ),
    zahl(
        "gain",
        "MASTER",
        0.0,
        1.5,
        Einheit::Faktor,
        true,
        "Summenlautstärke",
    ),
    zahl(
        "cue_gain",
        "KOPFH",
        0.0,
        1.5,
        Einheit::Faktor,
        true,
        "Kopfhörerlautstärke",
    ),
    zahl(
        "cue_mix",
        "CUE/MST",
        0.0,
        1.0,
        Einheit::Faktor,
        true,
        "Kopfhörer zwischen Vorhören (0) und Summe (1)",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jedes_control_beschreibt_sich_selbst() {
        // Der ganze Sinn des Katalogs: Wer fragt, bekommt eine Antwort, die
        // ohne Handbuch trägt.
        let alle = DECK.iter().chain(KANAL).chain(MASTER);
        for b in alle {
            assert!(!b.element.is_empty());
            assert!(!b.text.is_empty(), "{} hat keine Beschreibung", b.element);
            if b.art == Art::Zahl {
                assert!(
                    b.bereich.is_some(),
                    "{} ist eine Zahl ohne Bereich",
                    b.element
                );
            }
            if b.art == Art::Auswahl {
                assert!(
                    !b.auswahl.is_empty(),
                    "{} ist eine Auswahl ohne Optionen",
                    b.element
                );
            }
        }
    }

    #[test]
    fn namen_kommen_innerhalb_einer_gruppe_nur_einmal_vor() {
        for gruppe in [DECK, KANAL, MASTER] {
            let mut namen: Vec<_> = gruppe.iter().map(|b| b.element).collect();
            let vorher = namen.len();
            namen.sort_unstable();
            namen.dedup();
            assert_eq!(namen.len(), vorher, "doppelter Name in einer Gruppe");
        }
    }

    #[test]
    fn normierung_geht_hin_und_zurueck() {
        let b = &KANAL[0]; // trim, 0..2
        assert_eq!(b.aus_normiert(0.0), 0.0);
        assert_eq!(b.aus_normiert(1.0), 2.0);
        assert_eq!(b.nach_normiert(1.0), 0.5);

        // Ein bipolares Control hat seine Mitte bei 0.5 normiert — das ist der
        // Punkt, an dem ein MIDI-Regler in der Raste steht.
        let filter = KANAL.iter().find(|b| b.element == "filter").unwrap();
        assert_eq!(filter.nach_normiert(0.0), 0.5);
        assert_eq!(filter.aus_normiert(0.5), 0.0);
    }

    #[test]
    fn werte_ausserhalb_werden_begrenzt_statt_abgelehnt() {
        // Ein MIDI-Regler auf Anschlag meint das Maximum, keinen Fehler.
        let fader = KANAL.iter().find(|b| b.element == "fader").unwrap();
        assert_eq!(fader.begrenzen(1.7), 1.0);
        assert_eq!(fader.begrenzen(-0.2), 0.0);
    }

    #[test]
    fn es_gibt_genau_so_viele_hot_cues_wie_im_deck() {
        assert!(hot_cue_beschreibung(HOT_CUES - 1).is_some());
        assert!(hot_cue_beschreibung(HOT_CUES).is_none());
        assert_eq!(hot_cue_beschreibung(0).unwrap().element, "cue1");
    }

    #[test]
    fn nur_gelesene_controls_sind_auch_als_solche_markiert() {
        let dauer = DECK.iter().find(|b| b.element == "duration").unwrap();
        assert!(
            !dauer.schreibbar,
            "die Länge eines Tracks lässt sich nicht setzen"
        );
    }
}
