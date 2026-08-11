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

## Was jetzt zu tun ist

Nichts. Die Entscheidung, ob und wann angeschlossen wird, kann später fallen,
solange zwei Dinge eingehalten werden:

1. **Keine (A)GPL-Abhängigkeiten aufnehmen**, solange die Frage offen ist.
2. **Die Steuerung des Decks als Schnittstelle denken**, nicht als UI-Innenleben
   — dann ist der MCP-Server später eine dünne Hülle statt eines Umbaus.

Beides ist heute erfüllt.
