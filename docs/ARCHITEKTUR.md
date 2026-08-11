# Architektur (Entwurf)

Skizze, keine festgelegte Umsetzung. Dient dazu, die Schnittstellen früh zu
trennen — besonders die zwischen DJ-Kern und Suno-Anbindung.

## Bausteine

```
                ┌──────────────────────────┐
                │           UI             │
                │  Decks · Mixer · Library │
                └────────────┬─────────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
┌───────▼───────┐   ┌────────▼────────┐   ┌───────▼────────┐
│  Audio-Engine │   │     Library     │   │  Track-Quellen │
│ Decks, Mixer  │   │  Metadaten, DB  │   │                │
│ EQ, Filter    │   │  Playlists      │   │  Datei-Import  │
│ Zeitstreckung │   │  Cues, Beatgrid │   │  Suno-Adapter  │
└───────────────┘   └─────────────────┘   └────────────────┘
        │                                          │
┌───────▼───────┐                          ┌───────▼────────┐
│    Analyse    │                          │   Suno API     │
│  BPM, Grid    │                          │  (extern)      │
│  Waveform     │                          └────────────────┘
└───────────────┘
```

## Leitgedanken

**Track-Quellen sind austauschbar.** Ein Deck bekommt Audio plus Metadaten und
interessiert sich nicht dafür, ob die Datei von der Platte kam oder gerade
generiert wurde. Der Suno-Adapter erfüllt dieselbe Schnittstelle wie der
Datei-Import.

**Generierung ist asynchron, Wiedergabe nicht.** Suno-Aufrufe dauern; sie laufen
in einer Queue und dürfen den Audio-Pfad nie blockieren. Ein Track wird erst
deckfähig, wenn er vollständig vorliegt.

**Analyse ist vom Abspielen entkoppelt.** BPM-Erkennung und Waveform-Berechnung
laufen außerhalb des Audio-Threads und schreiben ihr Ergebnis in die Library.

## Offene Fragen

- Web Audio API oder nativer Audio-Kern? (Latenz, Zeitstreckungsqualität)
- Welcher Zeitstreckungs-Algorithmus, damit Pitch-Änderungen brauchbar klingen?
- Wo landen generierte Tracks — nur lokal oder auch versioniert mit Prompt?
- Welches Suno-API-Tier, und wie werden Kosten/Rate-Limits sichtbar gemacht?
