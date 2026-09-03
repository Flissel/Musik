# Anschluss an VibeMind

Kann dieses Projekt später Teil von [VibeMind](https://github.com/Flissel/Vibemind_V1)
werden? Ja. Die Anschlussstelle steht dort schon, und dieses Dokument hält fest,
was das für Entscheidungen hier bedeutet.

Stand der Betrachtung: VibeMind V1, August 2026.

## Wie VibeMind Dinge anschließt

```
Voice · UI · Channels · API
          │
        Brain            Intent, Planung, Routing
          │  autorisierte Übergabe
      OpenFang           Agentenauflösung, Freigabe, Ausführung
          │
       Agenten
          │
   MCP-Tools (allowlisted)
          │
       Spaces
```

Zwei Muster sind für uns relevant:

**MCP-Server.** VibeMind spricht Werkzeuge über MCP an — `FastMCP` über stdio,
ein Python-Paket je Server, Lesewerkzeuge laufen frei, schreibende gehen durch
eine Freigabe. Vorbild ist `mcp/docker/`.

**Spaces.** Fachdomänen liegen unter `spaces/<name>/` mit `agents/`, `api/`,
`README.md` und `STATUS.md`. Vorbild ist `spaces/marketing/`.

Größere Komponenten hängen als Submodule in `repo-manifest.yaml`.

## Der naheliegende Weg

Der Rust-Kern bleibt, was er ist — die Audio-Engine. Darüber kommt eine dünne
Steuerfläche als MCP-Server:

| Werkzeug | Art | Zweck |
| --- | --- | --- |
| `analyze_track` | lesend | Tempo, Beatgrid, Spitzen — läuft heute schon |
| `library_search` | lesend | Tracks nach BPM, Tonart, Tags |
| `deck_state` | lesend | Position, Tempo, was liegt auf welchem Deck |
| `deck_load` | schreibend | Track auf ein Deck legen |
| `deck_play` / `deck_tempo` | schreibend | Wiedergabe steuern |
| `mixer_set` | schreibend | Fader, EQ, Filter |

**Die Sprachgrenze ist kein Problem.** VibeMind ist Python, dieses Projekt ist
Rust — MCP läuft über eine Prozessgrenze und ist genau dafür gedacht. Der Server
kann in Rust geschrieben sein oder als dünne Python-Schicht, die über einen
lokalen Socket mit der Engine redet.

## Was das für die Agenten-Schicht bedeutet

[AGENTEN.md](AGENTEN.md) entwirft ein Team aus Crowd-Sensor, Energie-Analyst,
Set-Planer, Track-Wähler, Generator und Mix-Engineer. **Diese Schicht sollte hier
nicht noch einmal gebaut werden.** Brain und OpenFang machen genau das —
Intent, Planung, Routing, Freigabe, Ausführung.

Die Arbeitsteilung wäre also:

- **Hier:** Audio-Engine, Analyse, Library, MCP-Werkzeuge
- **VibeMind:** die Agenten, die diese Werkzeuge benutzen, als Space `musik`

Das streicht einen ganzen Bauabschnitt. Phase A2 aus der Roadmap wird damit
von „Agenten-Framework bauen" zu „Space anlegen und Werkzeuge freischalten".

## Zwei Stolpersteine

### 1. Die Lizenzlage kippt zurück

⚠️ **VibeMind steht unter MIT und ist zur Veröffentlichung vorgesehen.**

Damit gilt die Entlastung aus [BAUSTEINE.md](BAUSTEINE.md) — „nicht kommerziell,
also ist GPL frei" — für diesen Weg **nicht mehr**. Copyleft greift bei
Weitergabe, und ein veröffentlichtes MIT-Projekt ist Weitergabe.

Konkret fielen wieder aus:

| Bibliothek | Warum |
| --- | --- |
| Rubber Band (GPL) | Zwänge das Gesamtprojekt unter GPL |
| aubio (GPL) | dito |
| Essentia (AGPL) | dito, zusätzlich bei Netzwerknutzung |
| CC-BY-NC-Samples | dürften nicht mitgeliefert werden |

**Der aktuelle Stand ist zum Glück sauber.** Alles, was heute im Repo hängt, ist
MIT-verträglich:

| Crate | Lizenz |
| --- | --- |
| `cpal` | Apache-2.0 |
| `symphonia` | MPL-2.0 — dateiweises Copyleft, in einem MIT-Projekt unproblematisch |
| `rustfft`, `blake3`, `serde`, `base64` | MIT/Apache-2.0 |

Zeitstreckung und Tempoerkennung sind selbst geschrieben. Das war ursprünglich
eine Notlösung, weil die Lizenzfrage noch offen war — und hält jetzt genau die
Tür offen, die es für VibeMind braucht.

**Empfehlung: permissiv bleiben.** Solange ein Anschluss an VibeMind denkbar
ist, ist Rubber Band die teure Option, auch wenn sie für den reinen Eigenbedarf
erlaubt wäre. Reicht die eigene WSOLA nicht, ist
[signalsmith-stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)
(MIT) der Weg, der beide Optionen offenlässt.

### 2. Freigabe-Pflicht und Livebetrieb vertragen sich nicht

In VibeMind laufen lesende MCP-Werkzeuge frei, schreibende durch eine Freigabe.
Für einen DJ-Kontext ist das eine harte Grenze: „Lege Track X auf Deck 2" ist
formal ein schreibender Aufruf, aber mitten in einem Set kann niemand auf eine
Bestätigung warten. Sechzehn Takte sind bei 128 BPM dreißig Sekunden.

Das ist kein Hindernis, sondern eine Entwurfsvorgabe:

- **Vorbereitende Aufrufe** (analysieren, suchen, Kandidaten generieren) dürfen
  durch die Freigabe.
- **Zeitkritische Aufrufe** (Fader, EQ, Loop, Cue) brauchen eine im Voraus
  erteilte Freigabe für die Dauer des Sets — ein Bereich, kein Einzelfall.

Das ist dieselbe Frage wie die offene Entscheidung „Vollautomat oder Assistent
am Pult", nur von der Sicherheitsseite betrachtet.

## Plattform

VibeMind zielt auf Windows 11. CPAL unterstützt dort WASAPI und, über ein
Feature-Flag, ASIO. Für den getrennten Cue-Ausgang aus
[PLAN.md](PLAN.md#1-der-cue-ausgang-zwingt-zu-einem-gerät-mit-vier-ausgängen)
führt unter Windows praktisch kein Weg an ASIO vorbei — das ist beim Kauf des
Interfaces mitzudenken.

## Die Voraussetzungen, nachgemessen

Die beiden Bedingungen von oben sind erfüllt, und zwar geprüft statt behauptet.

**Lizenzen.** Über alle 474 Pakete der `Cargo.lock`, aufgelöst gegen die
entpackten Quellen: 203× `MIT OR Apache-2.0`, 95× `MIT`, 43× `Apache-2.0 OR
MIT`, 12× `MPL-2.0` (Symphonia; dateiweises Copyleft, in einem MIT-Projekt
unproblematisch). **Kein einziges Copyleft ohne permissiven Zweig.** Drei
Pakete tragen „GPL" im Lizenzfeld — `r-efi` und `self_cell` —, alle drei
mehrfachlizenziert (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`, `Apache-2.0 OR
GPL-2.0-only`); man nimmt den permissiven Zweig.

**Schnittstelle statt UI-Innenleben.** Der Steuerraum ist die einzige Art, die
Anlage zu bedienen — auch die Oberfläche geht durch ihn. Die MCP-Brücke ist
deshalb heute schon eine dünne Hülle.

## Was zwischen hier und einem Space liegt

Vier Dinge, im echten `Vibemind_V1` nachgesehen, nicht aus diesem Dokument
abgeleitet:

| | Stand |
| --- | --- |
| `spaces/musik/` | existiert nicht — es gibt nur `marketing` |
| Eintrag in `repo-manifest.yaml` | keiner |
| Freigabe-Gatter für schreibende Werkzeuge | **fehlt ganz** — alle 16 laufen frei |
| FastMCP-Herkunft | hier `fastmcp`, dort `mcp.server.fastmcp` |

Das dritte ist das einzige mit Substanz; die anderen drei sind Umzug.

### Das Freigabe-Modell für ein Pult

VibeMind lässt lesende Werkzeuge frei und schickt schreibende durch
`approval.require_approval` — bei `mcp/docker/` mit Default-Deny und einer
Erlaubnisliste in der Umgebung. Für ein DJ-Pult ist „jeder Schreibzugriff
einzeln" unbrauchbar: Sechzehn Takte sind bei 128 BPM dreißig Sekunden, und
niemand bestätigt mitten in einer Blende.

Die Auflösung ist keine Ausnahme vom Modell, sondern eine andere Körnung —
**Freigabe als Bereich für die Dauer eines Sets**, nicht je Aufruf:

| Stufe | Was | Warum |
| --- | --- | --- |
| **Frei** | `musik_status`, `musik_search`, `list`, `get`, `plan`, `master.events` | Auskunft ändert nichts |
| **Set-Freigabe** | Fader, EQ, Filter, Crossfader, Loop, Cue, `uebergang`, `ramp`, `schedule`, `when` | Zeitkritisch; einmal erteilt, gilt für das Set |
| **Einzeln** | `record`/`record_stop`, `queue_add <pfad>`, `deckN.load <pfad>`, `master.arc` | Berührt Dateisystem oder die Dramaturgie |
| **Nie ohne Mensch** | Generator-Aufrufe | Kosten Geld, und zwar je Aufruf |

Die Set-Freigabe hat einen Anfang und ein Ende und gehört ins Protokoll: Wer
sie erteilt hat und wann sie ausläuft, steht in der Mitschrift wie jede andere
Zeile. Ein Set ohne Ende wäre eine Dauererlaubnis mit anderem Namen.

## Was der Musik-App zum Ziel fehlt

Ziel ist, dass Agenten die Musik machen. Gemessen daran fehlt Folgendes — die
Liste ist nach Abhängigkeit sortiert, nicht nach Aufwand.

**1. Der Generator ist nicht angefangen.** In `crates/` steht dazu keine Zeile.
Der ganze rechte Ast des Entwurfs in [AGENTEN.md](AGENTEN.md) — `[Generator] →
Suno/ElevenLabs` — ist Papier. Das ist die größte Lücke zwischen dem, was das
Projekt tut, und dem, wofür es gebaut wird.

**2. Suno gibt es nicht zu haben.** Kein öffentlicher Endpunkt, keine Doku, kein
Preismodell, kein Termin — siehe [APIS.md](APIS.md). Das Partner-Programm ist
seit Juli 2026 offen und die Bewerbung einen Versuch wert, aber darauf lässt
sich nicht planen. Reverse-engineerte Dritt-„APIs" scheiden aus; das realistische
Risiko ist ein gesperrter Account.

**Konsequenz:** Der Generator kommt hinter eine Adapter-Schnittstelle, und der
erste Adapter ist **ElevenLabs Music** — dokumentierter `/music/compose`, die
klarste Lizenzlage im Feld. Suno wird später ein weiterer Adapter, kein Umbau.
Das steht so schon in [ARCHITEKTUR.md](ARCHITEKTUR.md); neu ist nur, dass es
jetzt gebaut werden müsste.

**3. Die Signale haben keine Quelle.** Der Raum steuert Bogen und Auswahl, aber
gemeldet wird von Hand. Der Crowd-Sensor aus dem Entwurf existiert nicht. Für
einen Space ist das weniger schlimm als es klingt: VibeMind bringt Kanäle mit,
und ein Signal von dort ist ein `set master.room signal1 …` wie jedes andere.

**4. Zwei Decks sind fest verdrahtet.** Der Plan will vier. Solange Stems nicht
von Platte streamen (rund 500 MB je Deck), wäre der vierte ohnehin nicht
finanzierbar — die beiden hängen zusammen.

**5. Die Analyse ist nie gegen echte Musik mit gehörter Wahrheit gelaufen.**
Gebautes Material hat zuletzt zehn von 23 Namen gefunden; es ersetzt kein Ohr.
Ein Agent, der auf `section` und `energie` plant, plant auf ungeprüften Zahlen.

**6. Zwei unabhängige Modelle haben noch nie zugleich aufgelegt** (P2 Stufe 2).
Braucht API-Zugang und kostet Geld — das ist eine Entscheidung, keine Aufgabe.

Nicht auf dieser Liste, weil sie nichts blockieren: Pitch Bend, Reverb, FLAC,
MIDI, und eine Named Pipe für Windows statt der Rückschleife.

## Die Reihenfolge

Die Einzelheiten stehen in drei Spezifikationen, nicht hier:

- **[FREIGABE.md](FREIGABE.md)** — das Gatter: sieben Klassen, die Set-Freigabe
  als Datei, zwölf Tests, die dafür stehen müssen.
- **[SPACE.md](SPACE.md)** — Aufbau des Space, sieben Hard Rules, der Umzug der
  Brücke, der Manifest-Eintrag.
- **[GENERATOR.md](GENERATOR.md)** — der Adapter, die Ablage samt Prompt, die
  Kostenbremse.


Sicherheit zuerst, dann Umzug, dann die Agenten, deren Werkzeuge schon stehen,
und zuletzt der, der Geld kostet.

| | Schritt | Wo | Hängt an |
| --- | --- | --- | --- |
| **1** | Freigabe-Gatter mit den vier Stufen, Default-Deny, Set-Freigabe mit Ablauf | dieses Repo, `mcp/` | — |
| **2** | Brücke auf `mcp.server.fastmcp` und in die Form `mcp/musik/` mit `server.py`, `approval.py`, `tests/` | dieses Repo | 1 |
| **3** | `spaces/musik/` mit `README.md`, `AGENTS.md`, `STATUS.md`, `agents/`, `api/` | VibeMind | 2 |
| **4** | Eintrag in `repo-manifest.yaml`, dieses Repo als Submodul mit Pin | VibeMind | 3 |
| **5** | **Mix-Engineer** als erster Agent | Space | 4 |
| **6** | **Track-Wähler** | Space | 5 |
| **7** | **Set-Planer** und **Guardrail** | Space | 6 |
| **8** | Generator-Adapter, erster Anbieter ElevenLabs | dieses Repo | Budget |

**Warum der Mix-Engineer zuerst.** Seine Werkzeuge sind fertig und gemessen: das
Repertoire aus fünf Griffen, der Zeitplan auf dem Beatgrid, die Zurückhaltung,
die Mitschrift und der Kritiker, der jeden Übergang nachmisst. Er ist der
einzige Agent, dessen Arbeit sich am selben Abend prüfen lässt — und er ist der
erste Fall, an dem sich zeigt, ob die Set-Freigabe im Betrieb trägt.

**Warum der Generator zuletzt.** Er hängt an einem Anbieter, an Geld und an
einer Lizenzfrage, die keiner von uns entscheidet. Und die harte Grenze aus
[AGENTEN.md](AGENTEN.md) bleibt: Ein generierter Track braucht Zehner von
Sekunden bis Minuten, ein Dancefloor entscheidet in Sekunden. Generiert wird
**vorausschauend** — Kandidaten für wahrscheinliche nächste Zustände —, und die
Crowd steuert die Auswahl aus fertigem Material. Wer auf Echtzeit-Generierung
wartet, baut ein System, das immer zu spät kommt.

**Zwei Dinge, die dabei nicht vergessen werden dürfen.** Generierte Tracks
brauchen einen Ort samt ihrem Prompt — sonst weiß hinterher niemand, wie ein
Stück entstanden ist. Und Kosten und Rate-Limits gehören sichtbar gemacht, im
Steuerraum wie alles andere; ein Agent, der unbemerkt Geld ausgibt, ist ein
Fehler mit Rechnung.
