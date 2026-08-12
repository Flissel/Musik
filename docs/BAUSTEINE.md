# Technische Bausteine

Was es an fertigen Bibliotheken gibt, damit wir nichts nachbauen, was es schon
gibt. Recherchestand August 2026, nicht selbst getestet.

## Lizenzen — entschärft, nicht erledigt

**Das Projekt ist nicht kommerziell.** Damit fällt die Sperre weg, die den
Audio-Bereich sonst prägt: (A)GPL-Bibliotheken mit kommerzieller Zweitlizenz
sind hier nutzbar.

Der Grund ist präziser als „nicht kommerziell": **Copyleft greift bei
Weitergabe, nicht bei Nutzung.** Solange das Tool auf dem eigenen Rechner läuft
und nicht verteilt wird, entstehen aus GPL-Abhängigkeiten überhaupt keine
Pflichten.

**Aktueller Stand: reine Eigennutzung, Weitergabe nicht geplant.** Damit ist die
Bibliothekswahl formal frei — auch Rubber Band. Eine `LICENSE`-Datei braucht das
Repo in diesem Zustand nicht.

⚠️ **Trotzdem ist Zurückhaltung geboten.** Ein Anschluss an VibeMind (MIT,
zur Veröffentlichung vorgesehen) steht im Raum, und der wäre Weitergabe — dann
gilt die Entlastung nicht mehr. Solange diese Frage offen ist, ist jede
GPL-Abhängigkeit eine Tür, die sich hinter einem schließt. Siehe
[VIBEMIND.md](VIBEMIND.md#1-die-lizenzlage-kippt-zurück).

| Bibliothek | Lizenz | Für dieses Projekt |
| --- | --- | --- |
| Essentia | AGPL | Nutzbar — siehe AGPL-Hinweis unten |
| Rubber Band | GPL + kommerziell | Nutzbar |
| aubio | GPL | Nutzbar |
| JUCE | GPL + kommerziell | Nutzbar |
| SoundTouch | LGPL | Nutzbar |
| librosa | ISC | Frei |
| miniaudio | Public Domain / MIT-0 | Frei |
| CPAL (Rust) | MIT / Apache-2.0 | Frei |
| Symphonia (Rust) | MPL-2.0 | Frei |
| Demucs | MIT (Code) | Modellgewichte separat prüfen |
| Mixxx | GPLv2+ (Binary faktisch GPLv3+) | Nutzbar — als Referenz lesen, nicht einbetten: [MIXXX.md](MIXXX.md) |

Zwei Punkte bleiben trotzdem stehen:

⚠️ **Wenn das Tool je weitergegeben wird** — auch kostenlos, auch nur an
Freunde, auch als Open Source — greift die GPL: Der eigene Code müsste dann
ebenfalls unter GPL stehen. Das ist kein Problem, aber eine Entscheidung, die
dann ansteht und nicht danach.

⚠️ **AGPL geht weiter als GPL.** Sie löst schon bei Nutzung über ein Netzwerk
aus, nicht erst bei Weitergabe. Relevant wird das, wenn die Agenten-Schicht aus
[AGENTEN.md](AGENTEN.md) je als Dienst läuft, auf den andere zugreifen — etwa
ein Crowd-Voting per Handy. Dann wäre Essentia im Stack ein Thema. `librosa`
(ISC) und `madmom` sind für Analyse die unkomplizierteren Wege.

Bei Samples bleibt eine Pflicht unabhängig davon bestehen: **CC BY verlangt
Namensnennung auch nicht-kommerziell.** Siehe [APIS.md](APIS.md#samples-und-audiomaterial).

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

Rust statt C++, weil der Stack damit klein und permissiv bleibt: CPAL,
Symphonia, ein eigener Stretcher. Dass GPL inzwischen offensteht (nicht
kommerziell), ändert daran nichts — die Wahl war zu dem Zeitpunkt richtig und
ist es geblieben. Sie macht nur den *Zwang* weg, alles selbst zu bauen.

## Time-Stretching und Pitch-Shifting

Kernstück jedes DJ-Tools: Tempo ändern, ohne dass die Tonhöhe mitwandert
(Keylock).

- **[Rubber Band Library](https://breakfastquay.com/rubberband/)** — Referenzqualität, GPL + kommerziell
- **[SoundTouch](https://www.surina.net/soundtouch/)** — LGPL, solide, weit verbreitet
- **[signalsmith-stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)** — MIT, modern
- Aktuell im Einsatz: **eigene WSOLA-Implementierung** in `crates/audio-core/src/stretch.rs`

Da das Projekt nicht kommerziell ist, steht **Rubber Band offen** — die beste
der drei Bibliotheken. Sie ist C++ und müsste per FFI angebunden werden.

Der Austausch lohnt aber erst, wenn die eigene WSOLA hörbar nicht reicht. Beim
DJ-typischen Bereich von ±8 % ist der Abstand klein; groß wird er bei starken
Faktoren und bei perkussivem Material. **Erst hören, dann tauschen** — die
Schnittstelle ist schmal genug, dass der Wechsel jederzeit möglich bleibt.

## BPM, Beatgrid, Tonart

- **[aubio](https://aubio.org/)** — Onset, Beat-Tracking, Pitch; C mit Python-Binding; GPL
- **[Essentia](https://essentia.upf.edu/)** — sehr umfangreich (Rhythmus, Tonalität, spektrale und High-Level-Deskriptoren), C++ mit Python-Wrapper, fertige Extraktoren mit JSON/YAML-Ausgabe; AGPL
- **[librosa](https://librosa.org/)** — Python, ISC, ideal zum Prototypen
- **[madmom](https://github.com/CPJKU/madmom)** — sehr gute Beat-/Downbeat-Erkennung

Alle vier sind jetzt nutzbar. Für Phase 2 ist `librosa` oder `madmom` der
erste Griff — nicht wegen der Lizenz, sondern weil die Analyse ohnehin offline
als eigener Schritt läuft und Python dort schneller zum Ergebnis führt als eine
FFI-Anbindung an den Rust-Kern.

Für Tonarterkennung liefert Essentia brauchbare Ergebnisse; als eigenständige
OSS-Alternative gibt es KeyFinder. Zu Essentia siehe den AGPL-Hinweis oben,
falls die Agenten-Schicht je über Netzwerk erreichbar wird.

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

Zeitstreckung ist **selbst implementiert** (WSOLA, `stretch.rs`). Zum Zeitpunkt
der Umsetzung war die Lizenzfrage noch offen, und der Prototyp sollte nicht
daran hängen. Inzwischen steht Rubber Band offen — der Tausch bleibt eine
Option, sobald jemand beides gehört hat.

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
- BPM-Analyse offline mit `librosa` oder `madmom`, Ergebnis neben dem Track ablegen
- Wenn die WSOLA-Qualität bei ±8 % nicht reicht: Rubber Band per FFI anbinden

## Quellen

- [ESSENTIA: an Audio Analysis Library for Music Information Retrieval](https://records.sigmm.org/2014/03/20/essentia-an-open-source-library-for-audio-analysis/)
- [stem-separation – GitHub Topics](https://github.com/topics/stem-separation)
- [Audio Development Tools (ADT) – Sammlung](https://github.com/Yuan-ManX/audio-development-tools)
- [traktor-nml-utils](https://github.com/wolkenarchitekt/traktor-nml-utils)
