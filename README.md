# Musik

Traktor-artiges DJ-/Musik-Tool, das generative Musik als gleichberechtigte
Quelle behandelt — mit dem Fernziel eines Agenten-Teams, das Musik aus den
Reaktionen der Crowd formt.

Status: **Phase 0.** Ein Deck läuft — Wiedergabe, Tempo mit und ohne Keylock,
Springen. Nativ in Rust.

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
| [docs/TRAKTOR-REFERENZ.md](docs/TRAKTOR-REFERENZ.md) | Was Traktor Pro 4 kann, was wir davon übernehmen |
| [docs/BAUSTEINE.md](docs/BAUSTEINE.md) | Fertige Bibliotheken für Audio, Analyse, Stems — inkl. Lizenzfallen |
| [docs/APIS.md](docs/APIS.md) | Generierungs-APIs und Sample-Quellen, Zugangsstatus |
| [docs/AGENTEN.md](docs/AGENTEN.md) | Multi-Agenten-Team und Crowd-Feedback-Schleife |
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

| Phase | Inhalt | Abhängig von |
| --- | --- | --- |
| ~~0~~ | ~~Stack-Entscheidung per Prototyp: ein Deck, Play/Pause/Pitch mit Keylock~~ ✅ | — |
| 1 | Zweites Deck, Crossfader, EQ, Filter — ein Mix ist durchgängig spielbar | 0 |
| 2 | BPM-Analyse, Beatgrid (inkl. manueller Korrektur), Sync | 1 |
| 3 | Waveforms, Cue-Points, Loops | 2 |
| 4 | Library: Import inkl. Traktor-`collection.nml`, Metadaten, Suche, Playlists | 2 |
| 5 | Generierungs-Adapter: Prompt → Track → Deck, asynchron mit Queue | 4, API-Zugang |
| 6 | Stems (beim Import vorberechnet), Stem-Deck-Modus | 4 |
| 7 | Crowd-Signale einsammeln und visualisieren — noch ohne Steuerung | 4 |
| 8 | Agenten-Team: Energie-Analyse, Set-Planung, Übergangssteuerung | 5, 7 |

Phase 5 hängt am API-Zugang, Phase 7 kann davon unabhängig laufen — Crowd-Signale
lassen sich auch gegen eine reine Datei-Library testen.

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

Die Zeitstreckung (WSOLA) ist selbst implementiert. Rubber Band ist besser und
steht inzwischen offen (GPL, nicht kommerzielles Projekt) — der Tausch lohnt
aber erst, wenn die eigene Variante hörbar nicht reicht. Details in
[docs/BAUSTEINE.md](docs/BAUSTEINE.md#phase-0-was-gebaut-ist).

UI ist noch offen und kommt frühestens mit Phase 1.

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

Zwei Dinge bleiben trotzdem zu beachten, Details in
[docs/BAUSTEINE.md](docs/BAUSTEINE.md#lizenzen--entschärft-nicht-erledigt):

- Wird das Tool je weitergegeben, auch kostenlos, müsste der eigene Code unter
  GPL stehen.
- AGPL (Essentia) löst schon bei Netzwerknutzung aus — relevant, falls die
  Agenten-Schicht je als Dienst läuft.

## Offene Entscheidungen

1. **Wieviel Kontrolle behält der Mensch am Pult?** Vollautomat oder Assistent —
   prägt das Agenten-Design.
2. **UI-Schicht.** Frühestens ab Phase 1.

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

Für API-Keys siehe [`.env.example`](.env.example).
