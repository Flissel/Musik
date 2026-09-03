# Generator: wenn nichts Passendes existiert

Spezifikation. Sie beschreibt den Teil, um dessentwillen das Projekt „Musik"
heißt und der als einziger noch gar nicht angefangen ist.

Stand: Entwurf, September 2026. In `crates/` steht dazu keine Zeile.

## Die Lage, kurz

**Suno hat keine öffentliche API.** Kein Endpunkt, keine Doku, kein Preismodell,
kein Termin — siehe [APIS.md](APIS.md). Es gibt seit Juli 2026 ein
Partner-Programm; die Bewerbung ist einen Versuch wert, aber darauf lässt sich
nicht planen. Reverse-engineerte Dritt-„APIs" scheiden aus: Das realistische
Risiko ist ein gesperrter Account.

Daraus folgt die ganze Bauform: **ein Adapter, mehrere Anbieter, Suno später
einer von ihnen.** Erster Adapter ist ElevenLabs Music — dokumentierter
`/music/compose`, die klarste Lizenzlage im Feld.

## Die harte Grenze, noch einmal

Aus [AGENTEN.md](AGENTEN.md), weil sie hier alles bestimmt:

> Ein Dancefloor entscheidet in **Sekunden**. Ein generierter Track braucht
> **Zehner von Sekunden bis Minuten**.

Die Schleife „Crowd reagiert → Agent generiert → Track spielt" schließt sich
nicht in Echtzeit, und keine Optimierung ändert das. Was trägt:

1. **Vorausschauend erzeugen.** Kandidaten für wahrscheinliche nächste Zustände,
   bevor sie eintreten. Die Crowd steuert dann die *Auswahl* aus Fertigem.
2. **Echtzeit nur für das, was schnell ist** — EQ, Filter, Loops, Stem-Muting.
3. **Erzeugen ist die langsame Schicht.** Sie formt die nächste halbe Stunde.

**Ein Track wird erst deckfähig, wenn er vollständig vorliegt.** Kein Streamen
in ein laufendes Deck, keine halben Dateien. Das ist keine Bequemlichkeit: Die
Analyse braucht den ganzen Track für Grid, Tonart und Gliederung, und ein Deck
ohne Grid kann nicht synchronisiert werden.

## Die Schnittstelle

Ein Trait in `crates/generator/`, damit der Rest der Anlage nichts über
Anbieter weiß.

```rust
/// Ein Anbieter, der aus einer Beschreibung Musik macht.
pub trait Erzeuger: Send + Sync {
    fn name(&self) -> &str;
    /// Beauftragt ein Stück. Kehrt sofort zurück — nichts hier wartet.
    fn beauftragen(&self, auftrag: &Auftrag) -> Result<AuftragsNummer, Fehler>;
    /// Fragt nach. Kein Warten, kein Blockieren.
    fn nachsehen(&self, nummer: &AuftragsNummer) -> Result<Stand, Fehler>;
    /// Was ein Auftrag kostet, soweit der Anbieter es sagt.
    fn kosten(&self, auftrag: &Auftrag) -> Option<Kosten>;
}

pub enum Stand {
    Laeuft { seit: Duration },
    Fertig { pfad: PathBuf },
    Gescheitert { grund: String },
}
```

`beauftragen` und `nachsehen` sind getrennt, weil jeder Anbieter asynchron ist
und keiner dieselbe Wartesemantik hat. Ein `erzeuge_und_warte` gäbe es nur, um
es an genau einer Stelle wieder aufzubrechen.

## Was ein Auftrag mitbringt

```rust
pub struct Auftrag {
    /// Was gewünscht ist, in Worten.
    pub beschreibung: String,
    /// Zieltempo. Ohne das ist der Track für ein Set unbrauchbar.
    pub bpm: Option<f32>,
    /// Zieltonart, für harmonisches Mischen.
    pub tonart: Option<Tonart>,
    pub sekunden: Option<f64>,
    /// Für Wiederholbarkeit — derselbe Seed, dasselbe Stück.
    pub seed: Option<u64>,
    /// Nur Instrumental.
    pub ohne_stimme: bool,
}
```

**BPM und Tonart gehören in den Auftrag, nicht in die Beschreibung.** Ein
Anbieter, der sie als Parameter kennt, trifft sie besser als einer, der sie aus
Prosa herauslesen soll — und wo er sie nicht kennt, hängt der Adapter sie an den
Text an. Der Unterschied ist prüfbar: Die eigene Analyse misst hinterher nach,
was tatsächlich herauskam.

## Wo das Ergebnis landet

Neben der Datei liegt, **wie es entstanden ist**. Sonst weiß hinterher niemand,
warum ein Stück klingt, wie es klingt.

```text
/musik/erzeugt/2026-09-03T21-14-07-elevenlabs-a3f9/
  stueck.wav
  auftrag.json      Beschreibung, BPM, Tonart, Seed, Anbieter, Modell
  antwort.json      Was der Anbieter zurückgab, roh
  gemessen.json     Was die eigene Analyse daraus liest
```

`gemessen.json` ist der interessante Teil: **Bestellt ist nicht geliefert.** Wer
128 BPM in A-Moll bestellt und 126,4 BPM in F-Dur bekommt, muss das sehen — und
zwar bevor der Track im Set liegt. Über viele Aufträge wird daraus eine
Trefferquote je Anbieter, und das ist die Zahl, nach der man einen aussucht.

Das Verzeichnis ist inhaltsadressiert genug, um zweimal dasselbe zu erkennen,
und lesbar genug, um von Hand nachzusehen. Ein Datenbankeintrag käme dazu, wenn
die Sammlung ihn braucht — die Datei bleibt die Wahrheit.

## Geld

Das Einzige an diesem Baustein, das echten Schaden anrichten kann.

- **Jeder Aufruf kostet.** Die Klasse `erzeugen` steht deshalb in
  [FREIGABE.md](FREIGABE.md) auf *nie ohne Mensch* und ist über die
  Freigabe-Datei **nicht erreichbar**. Ein Test hält das fest.
- **Eine Obergrenze je Abend**, in Aufträgen und in Geld, in der Konfiguration.
  Erreicht heißt: kein weiterer Auftrag, mit Meldung.
- **Sichtbar im Steuerraum**, wie alles andere:

| Control | Art | Bedeutung |
| --- | --- | --- |
| `master.erzeugt_laufend` | Zahl, r | Aufträge, die gerade laufen |
| `master.erzeugt_heute` | Zahl, r | Fertige Aufträge seit Mitternacht |
| `master.erzeugt_kosten` | Zahl, r | Was das gekostet hat, soweit bekannt |
| `master.erzeugt_grenze` | Zahl, rw | Obergrenze; 0 heißt aus |

**Ein Agent, der unbemerkt Geld ausgibt, ist ein Fehler mit Rechnung.** Deshalb
sind das Controls und keine Logzeilen: Sie stehen im Pult, in der Oberfläche und
in `musik_status`, und ein Mensch sieht sie, ohne zu suchen.

## Der Weg hinein

Ein fertiger Track ist eine Datei wie jede andere:

```text
Auftrag → Anbieter → Datei → analysieren → Sidecar → Sammlung → Deck
```

Kein Sonderweg. Ein erzeugter Track bekommt dasselbe Beatgrid, dieselbe Tonart
und dieselbe Gliederung wie einer von der Platte, und derselbe `musik-schneiden`
könnte ihn zerlegen, wenn er zu lang wäre. Das ist die Zusage aus
[ARCHITEKTUR.md](ARCHITEKTUR.md) — *„ein Deck interessiert sich nicht dafür, ob
die Datei von der Platte kam oder gerade generiert wurde"* — und sie wird
eingehalten, indem der Generator nichts als Dateien produziert.

## Was gebaut wird

| Schritt | Inhalt | Fertig, wenn |
| --- | --- | --- |
| 1 | `crates/generator/` mit Trait, `Auftrag`, `Stand`, Ablage | Tests gegen einen Attrappen-Erzeuger laufen, der nach N Aufrufen „fertig" meldet |
| 2 | Warteschlange, die nichts blockiert | ein laufender Auftrag den Audio-Pfad nachweislich nicht berührt |
| 3 | ElevenLabs-Adapter | ein bestellter Track ankommt und `gemessen.json` danebenliegt |
| 4 | Controls und Grenze | `master.erzeugt_grenze 1` den zweiten Auftrag abweist |
| 5 | Werkzeug `musik_erzeugen` mit Bestätigung je Aufruf | es ohne Bestätigung nichts tut |

**Schritt 1 und 2 brauchen kein Konto und kein Geld.** Ein Attrappen-Erzeuger,
der nach ein paar Sekunden eine still erzeugte Datei zurückgibt, prüft die ganze
Mechanik: Warteschlange, Zustände, Ablage, Analyse, Weg in die Sammlung. Erst
Schritt 3 kostet etwas — und erst dort ist die Anbieterfrage zu entscheiden.

Das ist die Reihenfolge, in der ich es bauen würde: die Mechanik zuerst gegen
eine Attrappe, den Anbieter zuletzt. Andersherum hängt die Architektur an
Eigenheiten eines Anbieters, den es vielleicht nicht bleibt.

## Was diese Spezifikation nicht regelt

- **Welcher Anbieter am Ende genommen wird.** ElevenLabs ist die Empfehlung von
  heute; die Lizenzlage im Feld ändert sich schnell, und vor jeder Integration
  ist sie neu zu prüfen.
- **Wie ein Agent zu einer guten Beschreibung kommt.** Das ist Arbeit des
  Set-Planers und gehört in den Space, nicht in die Engine.
- **Rechte am Ergebnis.** Was mit einem erzeugten Stück geschehen darf, hängt am
  Anbieter und ist vor der ersten Veröffentlichung zu klären — nicht danach.
