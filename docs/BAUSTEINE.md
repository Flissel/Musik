# Technische Bausteine

Was es an fertigen Bibliotheken gibt, damit wir nichts nachbauen, was es schon
gibt. Recherchestand August 2026, nicht selbst getestet.

## ⚠️ Lizenzen zuerst lesen

Der Audio-Bereich ist voll von **(A)GPL-Bibliotheken mit kommerzieller
Zweitlizenz**. Das ist kein Randthema: Wer Essentia oder Rubber Band einbaut und
das Produkt später verkaufen will, steht vor der Wahl, den eigenen Code
offenzulegen oder eine Lizenz zu kaufen. Diese Entscheidung fällt **vor** der
Bibliotheksauswahl, nicht danach.

| Bibliothek | Lizenz | Für ein Closed-Source-Produkt |
| --- | --- | --- |
| Essentia | AGPL | Nein, ohne kommerzielle Lizenz |
| Rubber Band | GPL + kommerziell | Lizenz kaufen |
| aubio | GPL | Problematisch |
| SoundTouch | LGPL | Meist okay (dynamisch gelinkt) |
| librosa | ISC | Frei |
| miniaudio | Public Domain / MIT-0 | Frei |
| CPAL (Rust) | MIT / Apache-2.0 | Frei |
| JUCE | GPL + kommerziell | Lizenz kaufen |
| Demucs | MIT (Code) | Modellgewichte separat prüfen |

Siehe die offene Frage in [APIS.md](APIS.md#konkrete-nächste-schritte): Ob das
Produkt kommerziell wird, entscheidet hier mit.

## Audio-Ausgabe und Engine

Das ist die Phase-0-Entscheidung.

**Browser — Web Audio API**
Reicht für Decks, EQ, Filter, Gain und Routing. Kein Installationsaufwand, gut
zu debuggen, schnell zu bauen. Grenzen: Latenz ist nicht garantiert, exklusiver
Zugriff auf ein Audio-Interface (ASIO/CoreAudio, getrennter Cue-Ausgang für
Kopfhörer) ist im Browser nicht sauber möglich. **Wichtig:** `playbackRate`
ändert Tempo *und* Tonhöhe — für Pitch-unabhängiges Tempo braucht es zwingend
einen eigenen Time-Stretcher (siehe unten).

**Nativ**
- [`miniaudio`](https://github.com/mackron/miniaudio) — Single-Header C, gemeinfrei, minimaler Ballast
- [`CPAL`](https://github.com/RustAudio/cpal) — Rust, plattformübergreifend, permissive Lizenz
- `RtAudio` — C++, etabliert
- `JUCE` — volles Framework inkl. UI, aber GPL/kommerziell

Nativ gibt echte Low-Latency-Ausgabe und mehrere Ausgänge — die Voraussetzung
für Kopfhörer-Cue, also für ernsthaftes DJing.

**Einschätzung:** Wenn das Ding je auf einer echten Anlage laufen soll, führt
langfristig kein Weg an nativ vorbei — wegen des getrennten Cue-Ausgangs, nicht
wegen der Latenz. Für einen ersten Prototyp ist der Browser trotzdem der
schnellere Weg, und die Analyse-/Library-/Agenten-Schichten sind davon ohnehin
unabhängig.

## Time-Stretching und Pitch-Shifting

Kernstück jedes DJ-Tools: Tempo ändern, ohne dass die Tonhöhe mitwandert
(Keylock).

- **[Rubber Band Library](https://breakfastquay.com/rubberband/)** — Referenzqualität, GPL + kommerziell
- **[SoundTouch](https://www.surina.net/soundtouch/)** — LGPL, solide, weit verbreitet
- **[signalsmith-stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)** — MIT, modern, auch als WASM nutzbar

Für einen Browser-Prototyp ist `signalsmith-stretch` per WebAssembly der
pragmatischste Einstieg: gute Qualität ohne Lizenzfrage.

## BPM, Beatgrid, Tonart

- **[aubio](https://aubio.org/)** — Onset, Beat-Tracking, Pitch; C mit Python-Binding; GPL
- **[Essentia](https://essentia.upf.edu/)** — sehr umfangreich (Rhythmus, Tonalität, spektrale und High-Level-Deskriptoren), C++ mit Python-Wrapper, fertige Extraktoren mit JSON/YAML-Ausgabe; AGPL
- **[librosa](https://librosa.org/)** — Python, ISC, ideal zum Prototypen
- **[madmom](https://github.com/CPJKU/madmom)** — sehr gute Beat-/Downbeat-Erkennung

Für Tonarterkennung liefert Essentia brauchbare Ergebnisse; als eigenständige
OSS-Alternative gibt es KeyFinder.

**Praktischer Hinweis:** Beatgrid-Erkennung ist bei geradem 4/4-Material fast
gelöst und bei allem anderen mühsam. Traktor löst das über manuelle
Grid-Korrektur — die brauchen wir auch, egal wie gut der Detektor ist.

## Stem-Separation

Vortrainierte Modelle:

- **[Demucs](https://github.com/facebookresearch/demucs)** (`htdemucs_ft`) — aktueller Qualitätsmaßstab
- **[Open-Unmix](https://github.com/sigsep/open-unmix-pytorch)** (`umxhq`)
- **[Spleeter](https://github.com/deezer/spleeter)** (`spleeter:4stems`) — älter, dafür schnell

Alle liefern die vier Stems, die auch Traktor nutzt: Drums, Bass, Vocals, Other.

⚠️ **Nicht echtzeitfähig auf üblicher Hardware.** Traktors Echtzeit-Separation
(iZotope RX) ist genau deshalb ein Verkaufsargument. Für uns heißt das: **Stems
beim Import einmal vorberechnen** und neben dem Track ablegen. Im Deck fühlt
sich das identisch an, kostet nur Plattenplatz statt Latenz.

## Waveform-Darstellung

Kein fertiges Paket nötig — Peaks beim Import berechnen, mehrstufig ablegen
(Übersicht grob, Zoom fein), im Canvas/WebGL zeichnen. Die Berechnung gehört zur
Import-Analyse, nicht in den Abspielpfad. Für die Farbcodierung nach
Frequenzbändern (wie Traktor) braucht es zusätzlich eine grobe
Bandaufteilung pro Peak.

## Traktor-Import

- **[traktor-nml-utils](https://github.com/wolkenarchitekt/traktor-nml-utils)** — Python, liest und schreibt `collection.nml`

Damit lassen sich Sammlung, Cue-Points, Beatgrids und Playlists übernehmen. Ein
gutes frühes Feature: viel Nutzen für wenig Aufwand. Siehe
[TRAKTOR-REFERENZ.md](TRAKTOR-REFERENZ.md#datenformat-die-collectionnml).

## Empfehlung für Phase 0

Als Prototyp, um die Stack-Frage empirisch statt theoretisch zu klären:

1. Ein Deck im Browser, Web Audio API, Datei laden und abspielen
2. Tempo per `signalsmith-stretch` (WASM) ändern, Tonhöhe halten
3. BPM offline mit `librosa` oder `aubio` bestimmen, Ergebnis als JSON danebenlegen
4. Messen: Wie fühlt sich die Latenz an? Wie klingt der Stretcher bei ±8 %?

Fällt der Test durch, ist der native Weg belegt statt vermutet — und die
Analyse-Skripte aus Schritt 3 bleiben unverändert nutzbar.

## Quellen

- [ESSENTIA: an Audio Analysis Library for Music Information Retrieval](https://records.sigmm.org/2014/03/20/essentia-an-open-source-library-for-audio-analysis/)
- [stem-separation – GitHub Topics](https://github.com/topics/stem-separation)
- [Audio Development Tools (ADT) – Sammlung](https://github.com/Yuan-ManX/audio-development-tools)
- [traktor-nml-utils](https://github.com/wolkenarchitekt/traktor-nml-utils)
