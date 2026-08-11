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

**Entscheidung: nativ, in Rust.** Ausschlaggebend war der getrennte
Cue-Ausgang, nicht die Latenz — ohne Kopfhörer-Vorhören ist ernsthaftes DJing
nicht möglich, und im Browser gibt es das nicht sauber.

Rust statt C++ wegen der Lizenztabelle oben: CPAL, Symphonia und ein eigener
Stretcher sind durchgehend permissiv. Damit bleibt die offene Frage „wird das
kommerziell?" wirklich offen, statt vom Stack vorentschieden zu werden.

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

## Phase 0: was gebaut ist

Ein Deck als Rust-Workspace, steuerbar über eine CLI.

| Crate | Inhalt |
| --- | --- |
| `crates/audio-core` | Dekodierung, Zeitstreckung, Deck-Zustand, CPAL-Ausgabe |
| `crates/musik-cli` | Kommandozeile zum Fahren des Decks |

**Gewählte Abhängigkeiten** — bewusst wenige, alle permissiv:

| Crate | Lizenz | Wofür |
| --- | --- | --- |
| `cpal` | Apache-2.0 | Ausgabegerät, plattformübergreifend |
| `symphonia` | MPL-2.0 | Dekodierung: MP3, FLAC, WAV, AAC/M4A, OGG |
| `thiserror` / `anyhow` | MIT/Apache-2.0 | Fehlerbehandlung |

Zeitstreckung ist **selbst implementiert** (WSOLA, `stretch.rs`). Das war keine
Sparmaßnahme, sondern folgt aus der Lizenztabelle: Rubber Band ist GPL, und der
Prototyp sollte nicht an einer Lizenzentscheidung hängen, die noch offen ist.

**Architektur des Abspielpfads.** Der Track wird beim Laden komplett dekodiert
und einmalig auf die Samplerate des Geräts gebracht. Dadurch enthält der
Callback keine Ratenkonvertierung, keine Allokation, kein Lock — Steuerung läuft
ausschließlich über Atomics. Das sind genau die drei Dinge, die sonst Aussetzer
verursachen.

**Was verifiziert ist.** Elf Tests, unter anderem:

- Keylock hält die Tonhöhe bei 0.92×, 1.06× und 1.20× (gemessen über
  Nulldurchgänge, Abweichung < 13 Hz bei 440 Hz)
- Ohne Keylock wandert die Tonhöhe korrekt mit dem Tempo mit
- Die Zeitachse driftet nicht: 4 s Ausgabe bei 1.06× verbrauchen 4,24 s Quelle
- Dekodierung, Mono→Stereo und Resampling halten Länge und Tonhöhe

Nicht verifiziert ist der Klang auf einer echten Anlage — das braucht Ohren an
einer Soundkarte, nicht Tests. Ebenso offen: gemessene Latenz und das Verhalten
unter Last.

## Was als Nächstes ansteht

- Zweites Deck und Mixer (Phase 1) — dafür muss der Track im Callback
  austauschbar werden, ohne dort zu allokieren (Send-Back-Kanal für den alten
  `Arc`, damit die Freigabe außerhalb des Audio-Threads passiert)
- BPM-Analyse offline mit `aubio` oder `librosa`, Ergebnis neben dem Track ablegen
- Wenn die WSOLA-Qualität bei ±8 % nicht reicht: `signalsmith-stretch` (MIT)
  einbinden, bevor über eine Rubber-Band-Lizenz nachgedacht wird

## Quellen

- [ESSENTIA: an Audio Analysis Library for Music Information Retrieval](https://records.sigmm.org/2014/03/20/essentia-an-open-source-library-for-audio-analysis/)
- [stem-separation – GitHub Topics](https://github.com/topics/stem-separation)
- [Audio Development Tools (ADT) – Sammlung](https://github.com/Yuan-ManX/audio-development-tools)
- [traktor-nml-utils](https://github.com/wolkenarchitekt/traktor-nml-utils)
