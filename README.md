# Musik

Traktor-artiges DJ-/Musik-Tool, das generative Musik als gleichberechtigte
Quelle behandelt — mit dem Fernziel eines Agenten-Teams, das Musik aus den
Reaktionen der Crowd formt.

Status: **Konzeptphase.** Noch kein Code. Recherche, Zielbild und Roadmap
stehen.

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
| **0** | Stack-Entscheidung per Prototyp: ein Deck, Play/Pause/Pitch mit Keylock | — |
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

**Offen — das ist die Phase-0-Entscheidung.** Die Frage ist nicht Latenz,
sondern der getrennte Kopfhörer-Cue-Ausgang: den gibt es im Browser nicht
sauber, und ohne ihn ist ernsthaftes DJing nicht möglich.

- **Browser (Web Audio API)** — schneller Prototyp, kein Install, gut zu
  debuggen. Kein exklusiver Interface-Zugriff, kein zweiter Ausgang.
- **Nativ (Rust/C++)** — echte Low-Latency-Ausgabe, mehrere Ausgänge, mehr
  Aufwand.

Analyse-, Library- und Agenten-Schicht sind von dieser Entscheidung unabhängig
und können parallel entstehen. Konkreter Testaufbau zur Entscheidung in
[docs/BAUSTEINE.md](docs/BAUSTEINE.md#empfehlung-für-phase-0).

## Nicht-Ziele (vorerst)

- Kein DVS/Timecode-Vinyl
- Keine Controller-/MIDI-Anbindung in den ersten Phasen
- Kein Streaming-Dienst-Import (Spotify, Beatport)
- Keine Echtzeit-Stem-Separation — beim Import vorberechnen (siehe
  [docs/BAUSTEINE.md](docs/BAUSTEINE.md#stem-separation))

## Offene Entscheidungen

1. **Browser oder nativ?** → Phase 0
2. **Wird das Produkt kommerziell?** Davon hängt die gesamte Lizenzstrategie ab,
   und zwar rückwirkend teuer — bei den Bibliotheken (AGPL/GPL), bei den
   Samples (CC BY-NC) und bei den Generierungs-Anbietern (Suno und Udio haben
   ungeklärte Lizenzlage).
3. **Wieviel Kontrolle behält der Mensch am Pult?** Vollautomat oder Assistent —
   prägt das Agenten-Design.

## Setup

Kommt mit Phase 0, sobald der Stack steht. Für API-Keys siehe
[`.env.example`](.env.example).
