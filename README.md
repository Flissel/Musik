# Musik

Traktor-artiges DJ-/Musik-Tool, das generative Musik als gleichberechtigte
Quelle behandelt — mit dem Fernziel eines Agenten-Teams, das Musik aus den
Reaktionen der Crowd formt.

Status: **Phase 0 und 2.** Ein Deck läuft (Wiedergabe, Tempo mit und ohne
Keylock, Springen), und die Analyse-Pipeline erkennt Tempo, Beatgrid und
Wellenform-Spitzen. Nativ in Rust.

## Zielbild

Drei Ebenen, aufeinander aufbauend:

1. **Ein DJ-Werkzeug**, das sich wie Traktor bedienen lässt — Decks, Mixer,
   Beatgrid, Library.
2. **Generierte Tracks als normale Quelle.** Ein Deck unterscheidet nicht
   zwischen einer Datei von der Platte und einem Track, der gerade per Prompt
   entstanden ist.
3. **Ein Agenten-Team, das die Crowd liest.** Die Menge schreibt keine Noten,
   aber ihre Reaktion wird Eingabe in Auswahl, Übergang und Komposition.

Ebene 1 ist die Voraussetzung für alles andere. Ohne ein Tool, das mixt, gibt
es nichts, was Agenten steuern könnten.

## Dokumentation

| Dokument | Inhalt |
| --- | --- |
| **[docs/PLAN.md](docs/PLAN.md)** | **Vollständiger Bauplan der DJ-Software — Architektur, Phasen, Risiken** |
| [docs/TRAKTOR-REFERENZ.md](docs/TRAKTOR-REFERENZ.md) | Was Traktor Pro 4 kann, was wir davon übernehmen |
| [docs/BAUSTEINE.md](docs/BAUSTEINE.md) | Fertige Bibliotheken für Audio, Analyse, Stems — inkl. Lizenzfallen |
| [docs/APIS.md](docs/APIS.md) | Generierungs-APIs und Sample-Quellen, Zugangsstatus |
| [docs/AGENTEN.md](docs/AGENTEN.md) | Multi-Agenten-Team und Crowd-Feedback-Schleife |
| [docs/VIBEMIND.md](docs/VIBEMIND.md) | Anschluss an VibeMind — MCP, Arbeitsteilung, Lizenzfolgen |
| [docs/ARCHITEKTUR.md](docs/ARCHITEKTUR.md) | Bausteine und Schnittstellen |

## Aktuelle Blocker

**Suno hat keine öffentliche Entwickler-API.** Stand August 2026 gibt es nur ein
kuratiertes Partner-Programm mit Bewerbungsformular (seit Juli 2026) — keine
Endpunkte, keine Doku, kein Termin. Generierung ist damit vorerst nicht über
Suno machbar.

Konsequenz: Der Generierungs-Teil wird **hinter einer Adapter-Schnittstelle**
gebaut und startet mit einem erreichbaren Anbieter (Vorschlag: ElevenLabs
Music). Suno wird später ein weiterer Adapter, kein Umbau. Details und
Alternativenvergleich in [docs/APIS.md](docs/APIS.md).

## Roadmap

**Phase 0 und 2 stehen** ✅ — ein Deck mit Keylock, und die Analyse-Pipeline.

Der ausführliche Bauplan der DJ-Software mit Architektur, Abnahmekriterien und
Risiken liegt in **[docs/PLAN.md](docs/PLAN.md)**. Grobe Reihenfolge:

| Phase | Inhalt |
| --- | --- |
| 1 | Mehrdeck-Engine mit Mixer und Cue-Bus |
| ~~2~~ ✅ | ~~Analyse-Pipeline (BPM, Grid, Peaks)~~ |
| 3–5 | UI mit Waveforms · Sync und Beatmatching · Hot Cues und Loops |
| 6–7 | Library inkl. Traktor-Import · Effekte |
| 8–10 | Stems · Mitschnitt · MIDI-Controller |

Ab Phase 6 ist die Software als DJ-Werkzeug benutzbar.

Darauf setzen die beiden Schichten aus dem Zielbild auf:

| Phase | Inhalt | Abhängig von |
| --- | --- | --- |
| G1 | Generierungs-Adapter: Prompt → Track → Deck, asynchron mit Queue | 6, API-Zugang |
| A1 | Crowd-Signale einsammeln und visualisieren — noch ohne Steuerung | 6 |
| A2 | Agenten-Team: Energie-Analyse, Set-Planung, Übergangssteuerung | G1, A1 |

G1 hängt am API-Zugang, A1 nicht — Crowd-Signale lassen sich auch gegen eine
reine Datei-Library testen.

## Tech-Stack

**Nativ, in Rust.** Ausschlaggebend war der getrennte Kopfhörer-Cue-Ausgang,
nicht die Latenz — ohne Vorhören ist ernsthaftes DJing nicht möglich, und im
Browser gibt es das nicht sauber. Rust statt C++, weil der Stack damit klein und
permissiv bleibt — zum Entscheidungszeitpunkt war die Lizenzfrage noch offen,
und der Werkzeugkasten sollte sie nicht vorwegnehmen.

| Crate | Lizenz | Wofür |
| --- | --- | --- |
| `cpal` | Apache-2.0 | Audioausgabe |
| `symphonia` | MPL-2.0 | MP3, FLAC, WAV, AAC/M4A, OGG |
| `rustfft` | MIT/Apache-2.0 | STFT für die Onset-Erkennung |
| `blake3`, `serde`, `base64` | permissiv | Fingerabdruck und Sidecar |

Zeitstreckung (WSOLA) und Tempoerkennung sind selbst implementiert. Rubber Band
ist beim Stretching besser und steht inzwischen offen (GPL, nicht kommerzielles
Projekt) — der Tausch lohnt aber erst, wenn die eigene Variante hörbar nicht
reicht. Details in
[docs/BAUSTEINE.md](docs/BAUSTEINE.md#phase-0-was-gebaut-ist).

**UI: egui/eframe**, entschieden. Begründung und die verworfenen Alternativen
(Tauri, Electron) in [docs/PLAN.md](docs/PLAN.md#entschiedene-punkte).

## Nicht-Ziele (vorerst)

- Kein DVS/Timecode-Vinyl
- Keine Controller-/MIDI-Anbindung in den ersten Phasen
- Kein Streaming-Dienst-Import (Spotify, Beatport)
- Keine Echtzeit-Stem-Separation — beim Import vorberechnen (siehe
  [docs/BAUSTEINE.md](docs/BAUSTEINE.md#stem-separation))

## Entschieden

**Nativ in Rust** (Phase 0) und **nicht kommerziell.**

Die zweite Entscheidung entspannt die Lizenzlage deutlich: GPL-Bibliotheken wie
Rubber Band und aubio stehen offen, ebenso CC-BY-NC-Samples von Freesound. Der
genaue Grund ist enger als „nicht kommerziell" — Copyleft greift bei
*Weitergabe*, nicht bei Nutzung. Solange das Tool auf dem eigenen Rechner
bleibt, entstehen gar keine Pflichten.

**Aktueller Stand: reine Eigennutzung, keine Weitergabe geplant.** Damit gibt es
derzeit überhaupt keine Lizenzpflichten aus den Abhängigkeiten, und deshalb
liegt auch bewusst keine `LICENSE`-Datei im Repo.

Die folgenden zwei Punkte sind Merkposten für den Fall, dass sich das ändert —
Details in
[docs/BAUSTEINE.md](docs/BAUSTEINE.md#lizenzen--entschärft-nicht-erledigt):

- Wird das Tool je weitergegeben, auch kostenlos, müsste der eigene Code unter
  GPL stehen.
- AGPL (Essentia) löst schon bei Netzwerknutzung aus — relevant, falls die
  Agenten-Schicht je als Dienst läuft.

## Offene Entscheidungen

1. **Wieviel Kontrolle behält der Mensch am Pult?** Vollautomat oder Assistent —
   prägt das Agenten-Design.
2. **Audio-Interface mit vier Ausgängen.** Ohne getrennten Cue-Ausgang lässt
   sich Phase 1 nicht abnehmen — eine Hardware-, keine Softwarefrage. Warum
   zwei Geräte nicht gehen, steht in
   [docs/PLAN.md](docs/PLAN.md#1-der-cue-ausgang-zwingt-zu-einem-gerät-mit-vier-ausgängen).

## Setup

Voraussetzung: Rust (aktuelles Stable). Unter Linux zusätzlich die
ALSA-Header — `sudo apt install libasound2-dev`.

```sh
cargo build
cargo test
```

Ein Deck fahren:

```sh
cargo run -p musik-cli -- /pfad/zum/track.mp3
```

Danach im Prompt:

| Befehl | Wirkung |
| --- | --- |
| `p` | Play/Pause |
| `t +6` | Tempo +6 % (oder `t 1.06` als Verhältnis) |
| `k` | Keylock an/aus — Tonhöhe beim Tempowechsel halten |
| `s 30` | Auf Sekunde 30 springen |
| `i` | Status |
| `q` | Beenden |

Der Vergleich, um den es geht: `t +6` einmal mit und einmal ohne `k`. Mit
Keylock bleibt die Tonhöhe stehen, ohne wandert sie hoch wie beim
Plattenspieler.

Tracks analysieren — braucht **kein** Audiogerät:

```sh
cargo run -p musik-cli --bin musik-analyze -- ~/Musik/*.mp3
```

Liefert BPM, Beatgrid-Anker und Wellenform-Spitzen und legt sie als Sidecar
unter `.musik-analyse/` ab, adressiert über einen Hash des Audioinhalts. Ein
zweiter Lauf über dieselbe Datei liest aus dem Cache; ein umbenannter Ordner
entwertet die Analyse nicht.

Für API-Keys siehe [`.env.example`](.env.example).
