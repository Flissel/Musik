# Musik

Traktor-artiges DJ-/Musik-Tool. Decks, Mixer, Library — mit dem Ziel, später die
[Suno API](https://suno.com) für KI-Musikgenerierung direkt in den Workflow einzubinden.

Status: **Konzeptphase** — noch kein Code, nur Zielbild und Roadmap.

## Zielbild

Ein DJ-Werkzeug, das sich wie Traktor bedienen lässt, aber generative Musik als
erstklassige Quelle behandelt: Ein Deck kann einen Track aus der Library laden —
oder einen, der gerade erst per Prompt entsteht.

## Geplante Funktionen

### Kern (DJ)
- Zwei oder mehr Decks mit unabhängiger Wiedergabe
- Pitch/Tempo-Kontrolle, Keylock
- BPM-Erkennung und Beatgrid
- Sync zwischen Decks
- Crossfader, Kanal-EQ, Gain, Filter
- Cue-Points und Loops
- Waveform-Darstellung (Übersicht + Zoom)
- Track-Library mit Metadaten, Suche, Playlists

### Suno-Integration (später)
- Track per Prompt generieren und direkt auf ein Deck laden
- Generierte Tracks in der Library mit Prompt-Historie ablegen
- Stem-/Variantenerzeugung als Material für Übergänge
- Warteschlange für Generierungen, damit der Mix nicht blockiert

## Roadmap

| Phase | Inhalt |
| --- | --- |
| 0 | Setup, Tech-Stack-Entscheidung, Audio-Prototyp (ein Deck, Play/Pause/Pitch) |
| 1 | Zweites Deck, Crossfader, EQ — ein Mix ist durchgängig spielbar |
| 2 | BPM-Analyse, Beatgrid, Sync |
| 3 | Waveforms, Cue-Points, Loops |
| 4 | Library: Import, Metadaten, Suche, Playlists |
| 5 | Suno-API-Adapter: Prompt → Track → Deck |
| 6 | Generierungs-Queue, Prompt-Historie, Varianten |

## Tech-Stack

Noch offen. Vorschlag als Ausgangspunkt:

- **Audio:** Web Audio API — reicht für Decks, EQ, Filter und Zeitstreckung,
  läuft ohne Installation und ist gut zu debuggen.
- **UI:** TypeScript + React
- **Analyse (BPM/Beatgrid):** zunächst im Browser, bei Bedarf als separater Worker

Alternative, falls die Latenz im Browser nicht reicht: nativer Audio-Kern
(Rust/C++ mit CPAL bzw. JUCE) und die UI darüber.

Diese Entscheidung fällt in Phase 0.

## Nicht-Ziele (vorerst)

- Kein DVS/Timecode-Vinyl
- Keine Controller-/MIDI-Anbindung in den ersten Phasen
- Kein Streaming-Dienst-Import (Spotify, Beatport)

## Setup

Kommt mit Phase 0, sobald der Stack steht.
