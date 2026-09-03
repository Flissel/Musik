# Traktor als Referenz

Traktor Pro 4 (Native Instruments) ist das Zielbild für die DJ-Seite dieses
Projekts. Dieses Dokument hält fest, was Traktor kann — damit klar ist, was wir
nachbauen, was wir bewusst weglassen und was wir anders machen wollen.

Quelle: Herstellerangaben und Reviews zu Traktor Pro 4, siehe Abschnitt
[Quellen](#quellen). Nicht selbst nachgemessen.

## Deck-Modelle

Traktor hat bis zu vier Decks, und ein Deck ist nicht nur ein Abspieler — es hat
drei Betriebsarten:

| Modus | Was es tut |
| --- | --- |
| **Track** | Klassische lineare Wiedergabe einer Datei |
| **Remix** | Grid aus Samples/Loops, die synchron getriggert werden |
| **Stem** | Vier getrennte Spuren innerhalb eines Tracks, live zu- und abschaltbar |

Das ist die wichtigste Erkenntnis für unsere Architektur: **ein Deck ist ein
Container mit austauschbarem Innenleben**, kein Dateiabspieler. Genau da hängt
sich später auch die Generierung ein — ein viertes Deck-Modell.

## Kernfunktionen

**Echtzeit-Stem-Separation.** Traktor zerlegt einen beliebigen Track live in
Drums, Bass, Vocals und „Other", angetrieben von iZotope-RX-Algorithmen. Damit
lassen sich Acapellas spontan droppen oder Breakdowns bauen. Siehe
[BAUSTEINE.md](BAUSTEINE.md#stem-separation) — das in Echtzeit hinzubekommen ist
der technisch teuerste Punkt der ganzen Liste.

**Pattern Player.** Lädt Drum-Kits und erzeugt polyrhythmische Percussion-
Sequenzen direkt aus dem Deck heraus. Mitgeliefert werden Kits von Künstlern wie
Rebekah, Luke Slater, Len Faki, Chris Liebing, Dubfire.

**Effekte.** Über 40 Effekte, dazu Mixer FX, die *post-fader* liegen — wichtig
für weiche Übergänge, weil der Effekt nicht mit dem Fader weggezogen wird.

**Beatgrids und Sync.** Automatische BPM-Erkennung mit Beatgrid pro Track,
darauf aufbauend Sync zwischen Decks.

**Harmonisches Mixen.** Tonarterkennung, damit sich Tracks nicht beißen.

**Master-Kette.** iZotope Ozone Maximizer auf der Summe.

**Library.** Smartlists (regelbasierte Playlists), Sampler, Suche über die
Sammlung.

## Datenformat: die `collection.nml`

Traktors Sammlung liegt als XML-Datei (`.nml`). Sie enthält **keine Audiodaten**,
sondern:

- Pfade zu den Mediendateien auf der Platte
- Metadaten (Titel, Artist, Genre, …)
- BPM und Beatgrid
- Tonart
- 1–8 Hot Cues und Loop-Marker
- Playlists

Das ist für uns direkt relevant: **Traktor-Import ist ein realistisches Feature.**
Wer von Traktor kommt, bringt Jahre an Cue-Points mit. Es gibt fertige Parser,
z. B. [`traktor-nml-utils`](https://github.com/wolkenarchitekt/traktor-nml-utils)
(Python).

## Controller-Anbindung

Mappings liegen als `.tsi`-Dateien und werden im Controller Manager verwaltet.
Praktisch jeder Generic-MIDI-Controller lässt sich anlernen. Für uns vorerst ein
Nicht-Ziel, aber die Architektur sollte MIDI-Events nicht ausschließen.

## Was wir übernehmen — und was nicht

| Traktor-Feature | Priorität bei uns | Anmerkung |
| --- | --- | --- |
| 2 Decks, Track-Modus | **Phase 1** | Fundament |
| Crossfader, EQ, Gain, Filter | **Phase 1** | Fundament |
| BPM/Beatgrid, Sync | **Phase 2** | Ohne das kein brauchbarer Mix |
| Waveform, Cues, Loops | **Phase 3** | |
| Library + Traktor-Import | **Phase 4** | NML-Parser existiert bereits |
| Tonarterkennung | Phase 4 | Nice-to-have, kein Blocker |
| Stem-Modus | Später | Vorberechnet, nicht Echtzeit (s. u.) |
| 40+ Effekte | Später | Wenige gute schlagen viele mittelmäßige |
| Pattern Player | Später | Interessant als Agenten-Spielwiese |
| 4 Decks | Später | 2 reichen lange |
| Remix-Decks | Später | |
| MIDI/Controller | Nicht-Ziel (vorerst) | |
| DVS/Timecode-Vinyl | **Nicht-Ziel** | Riesiger Aufwand, enge Zielgruppe |

**Zur Stem-Separation:** Traktor macht das in Echtzeit. Wir sollten das *nicht*
direkt versuchen. Stems beim Import einmal vorberechnen und ablegen bringt
denselben Nutzen im Deck, ohne das Latenzproblem. Echtzeit ist eine
Optimierung, kein Startpunkt.

## Wo wir bewusst abweichen

Traktor ist ein geschlossenes Werkzeug für einen Menschen an zwei Decks. Unser
Unterschied liegt woanders:

1. **Generierte Tracks sind gleichberechtigte Quelle** — siehe [APIS.md](APIS.md)
2. **Die Crowd ist ein Eingabegerät** — siehe [AGENTEN.md](AGENTEN.md)

Alles, was Traktor besser kann und wir nie einholen, ist damit kein Problem —
solange diese zwei Achsen funktionieren.

## Quellen

- [Traktor Pro 4 – Native Instruments (Review, MusicTech)](https://musictech.com/reviews/dj/native-instruments-traktor-4-pro/)
- [Native Instruments releases Traktor Pro 4 – Decoded Magazine](https://www.decodedmagazine.com/native-instruments-releases-traktor-pro-4-with-incredible-new-features/)
- [Traktor Pro 4 Produktseite – DJ TechTools](https://store.djtechtools.com/products/traktor-pro-4)
- [Configuring MIDI Controller for Controlling Traktor – Native Instruments](https://www.native-instruments.com/ni-tech-manuals/traktor-pro-manual/en/configuring-midi-controller-for-controlling-traktor)
- [traktor-nml-utils – NML-Parser](https://github.com/wolkenarchitekt/traktor-nml-utils)
- [Import Traktor – Lexicon DJ (NML-Feldbeschreibung)](https://www.lexicondj.com/manual/import-traktor)
