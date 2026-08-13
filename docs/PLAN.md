# Plan: vollständige DJ-Software

Der Weg von einem Deck an der Kommandozeile (Phase 0, steht) zu einem Werkzeug,
mit dem man tatsächlich auflegen kann.

Ergänzt [TRAKTOR-REFERENZ.md](TRAKTOR-REFERENZ.md) (was das Ziel kann) um das
Wie. Aufwandsangaben sind relativ (S/M/L/XL), keine Kalenderzeit.

## Was „vollständig" heißt

**Dazu gehört:** vier Decks, Mixer mit EQ und Filter, getrennter
Kopfhörer-Cue, Beatgrid und Sync, Hot Cues und Loops, Waveform-Anzeige,
Library mit Suche und Playlists, Effekte, vorberechnete Stems, Mitschnitt,
MIDI-Controller.

**Dazu gehört nicht:** DVS/Timecode-Vinyl, Video, Karaoke,
Streaming-Dienst-Integration. Alles vier ist viel Arbeit für wenig Ertrag in
diesem Projekt.

Generierung und Agenten sind eine Schicht *darüber* und stehen in
[APIS.md](APIS.md) und [AGENTEN.md](AGENTEN.md). Dieser Plan bringt das
Fundament, auf dem sie aufsetzen.

## Die drei Entscheidungen, die alles andere bestimmen

Der Rest des Plans ist Fleißarbeit. Diese drei nicht — sie prägen die
Architektur, und sie nachträglich zu ändern ist teuer.

### 1. Der Cue-Ausgang zwingt zu einem Gerät mit vier Ausgängen

Vorhören auf dem Kopfhörer war der Grund für die native Entscheidung. Praktisch
heißt das: **Master L/R und Cue L/R müssen über dasselbe Audiogerät laufen.**

Zwei getrennte Geräte haben zwei Quarze. Die laufen minimal unterschiedlich
schnell, und der Versatz summiert sich — nach ein paar Minuten ist der
Kopfhörer hörbar gegen den Master verschoben. Das ließe sich nur mit
asynchroner Ratenkonvertierung im Abspielpfad auffangen, also genau der
Rechenstufe, die wir dort bewusst nicht haben.

Konsequenz für die Hardware: ein Interface mit mindestens vier Ausgängen, oder
ein DJ-Controller mit eingebauter Soundkarte. Das ist eine
Anschaffungsentscheidung, keine Softwarefrage — und sie steht vor Phase 1, weil
sich der Cue-Bus sonst nicht testen lässt.

Plattformseitig: unter Linux ALSA oder PipeWire, unter macOS CoreAudio, unter
Windows ASIO (CPAL hat dafür ein Feature-Flag). Aggregate Devices unter macOS
lösen das Taktproblem übrigens *nicht* — sie bündeln zwei physische Uhren.

### 2. Atomics reichen nicht mehr — es braucht eine Kommandoschlange

Phase 0 steuert das Deck über Atomics. Das trägt für Skalare wie Tempo und
Play-Zustand, aber nicht für „lade diesen Track" oder „setze acht Cue-Points".

Nötig ist eine **lock-freie SPSC-Schlange** vom Steuer- zum Audio-Thread
([`rtrb`](https://crates.io/crates/rtrb), MIT/Apache-2.0). Und eine zweite in
die Gegenrichtung, deren Zweck man leicht übersieht:

> Wird ein Track auf einem Deck ausgetauscht, darf der alte `Arc<Track>` **nicht
> im Audio-Thread fallen gelassen werden.** Der letzte Drop gibt hundert
> Megabyte frei — ein Syscall mitten im Callback, also genau der Aussetzer, den
> die ganze Architektur vermeiden soll. Der alte Arc geht deshalb über eine
> Rückschlange hinaus und stirbt außerhalb.

```
Steuer-Thread ──── Kommandos (rtrb) ────► Audio-Thread
              ◄─── Altlasten (rtrb) ─────
              ◄─── Zustand (Atomics) ────
```

### 3. Beatgrid ist mehr als BPM

Für Sync reicht eine Zahl nicht. Gebraucht wird:

- **Anker** — Position des ersten Downbeats in Frames
- **BPM**
- optional eine **Liste von Grid-Markern** für Material mit schwankendem Tempo
  (Live-Aufnahmen, alte Platten)

Und Sync braucht zwei Größen, nicht eine: **Tempo** *und* **Phase**. Zwei Decks
mit identischem BPM, aber verschobener Phase, klingen genauso falsch wie
unterschiedliches Tempo. Die Engine braucht dafür eine globale Beat-Uhr, an der
ein Deck als Tempo-Master hängt.

## Architektur

### Crates

```
crates/
  audio-core/    steht — Dekodierung, WSOLA, Deck, Beatgrid, Cues, Loops
  audio-engine/  steht — Mixer, Kanalzüge, Cue-Bus, AUX, Sync, Begrenzer
  analysis/      steht — BPM, Beatgrid, Waveform-Peaks, Sidecar-Cache
  library/       steht — SQLite, Suche, Playlists, Traktor-Import
  musik-cli/     steht — Deck, Analyse, Offline-Mix, Sammlung
  control/       steht — benannter Steuerraum, Fernsteuerung über Socket
  musik-app/     steht — Oberfläche (egui/eframe): Decks, Mixer, Plattenkiste
  Effekte, Stems und MIDI kommen in audio-engine bzw. analysis dazu.
```

`audio-core` behält seine Rolle als Deck-Innenleben; `audio-engine` ist alles
darüber. Die Trennung lohnt sich, weil `audio-engine` die Echtzeitregeln
durchsetzt und `analysis`/`library` sie bewusst nicht kennen müssen.

### Signalkette pro Kanal

```
Quelle ─► Trim ─► EQ (3-Band) ─► Filter (HP/LP) ─┬─► Kanalfader ─► Crossfader ─► Summe ─► Limiter ─► Ausgang 1/2
                                                 │
                                                 └─► Cue-Bus ────────────────────────────────────► Ausgang 3/4
```

**Quelle, nicht Deck.** Der Mixer kennt keine Decks, nur Quellen, die Stereo
liefern. Ein AUX-Eingang ist damit ein Kanal wie jeder andere — mit Trim, EQ,
Filter, Fader und Cue —, und die Generierung kann sich später an derselben
Stelle einhängen.

Drei Details, die man leicht falsch baut:

- **Mixer-FX liegen post-fader.** Zieht man den Fader zu, klingt der Effekt aus
  statt abzureißen. Das ist der Unterschied zwischen einem weichen und einem
  ruckeligen Übergang.
- **Der Cue-Abgriff liegt vor dem *Fader*** — nicht bloß vor dem Crossfader.
  Man bereitet einen Track im Kopfhörer vor, während sein Fader unten ist; läge
  der Abgriff dahinter, hörte man genau dann nichts.
- **Der Cue-Abgriff liegt hinter EQ und Filter**, weil man seine Klangeingriffe
  kontrollieren muss, bevor sie auf die Anlage gehen.

### AUX

Mikrofon, Drum-Machine, ein zweiter Rechner. Zwei Dinge unterscheiden AUX von
einem Deck:

- **Zuweisung Thru.** Ein Mikrofon darf nicht verschwinden, nur weil jemand den
  Crossfader bewegt.
- **Ringpuffer statt direktem Zugriff.** Aufnahme und Wiedergabe laufen in
  getrennten Callbacks mit getrennten Uhren. Ein lock-freier Ringpuffer
  entkoppelt beide; bleibt die Aufnahme zurück, gibt es Stille und einen
  gezählten Unterlauf statt eines Aussetzers.

### Threads

| Thread | Aufgabe | Regeln |
| --- | --- | --- |
| Audio-Callback | Decks lesen, mischen, Effekte, Ausgabe | Keine Allokation, kein Lock, kein Syscall |
| UI | Darstellung ~60 fps, Eingaben → Kommandos | Liest Zustand über Atomics |
| Lader | Datei dekodieren, resamplen → `Arc<Track>` | Darf blockieren |
| Analyse | BPM, Grid, Peaks, später Stems | Parallel, im Hintergrund |

## Datenhaltung

**Analyse-Ergebnisse als Sidecar, adressiert über den Inhalts-Hash.**

Nicht über den Dateipfad — sonst ist die Arbeit weg, sobald jemand einen Ordner
umbenennt. Der Hash wird über die dekodierten Audiodaten gebildet, nicht über
die Datei, damit ein geänderter ID3-Tag die Analyse nicht entwertet.

Inhalt: BPM, Grid-Anker und -Marker, Tonart, Loudness, Waveform-Peaks in
mehreren Auflösungen (grob für die Übersicht, fein für den Zoom).

**Library als SQLite** ([`rusqlite`](https://crates.io/crates/rusqlite), MIT).

Tabellen: `tracks`, `cues`, `loops`, `playlists`, `playlist_items`, `analysis`.

⚠️ **`tracks` braucht von Anfang an `license` und `attribution`.** CC BY
verlangt Namensnennung auch bei nicht-kommerzieller Nutzung, und das lässt sich
nicht rekonstruieren, wenn erst tausend Freesound-Samples ohne Herkunft im
Ordner liegen. Siehe [APIS.md](APIS.md#samples-und-audiomaterial).

## Phasen

| # | Ziel | Abnahmekriterium | Aufwand | Braucht |
| --- | --- | --- | --- | --- |
| 1 | Mehrdeck-Engine, Mixer, Cue-Bus, AUX | Code steht ✅ · Abnahme offen: Cue muss hörbar getrennt auf 3/4 liegen | M | 4-Kanal-Interface |
| ~~2~~ | ~~Analyse-Pipeline~~ ✅ | BPM, Grid und Tonart reproduzierbar als Sidecar, Peaks vorhanden | M | — |
| ~~3~~ | ~~UI-Grundgerüst~~ ✅ | Zwei Decks mit Wellenform und Grid, Mixer bedienbar, Sammlung durchsuchbar · Offen: Bildrate auf echter Hardware messen | L | 1, 2 |
| ~~4~~ | ~~Sync und Beatmatching~~ ✅ | Zwei Tracks bleiben über 5 Minuten im Takt — als Test abgesichert | M | 2 |
| ~~5~~ | ~~Hot Cues und Loops~~ ✅ | 8 Cues pro Deck, Loop beat-quantisiert, in der Oberfläche bedienbar | M | 4 |
| ~~6~~ | ~~Library~~ ✅ | Import, Suche nach Text, Tempo und Tonart, Playlists, Traktor-`collection.nml` | L | 2 |
| ~~7~~ | ~~Effekte~~ ✅ | Vier Effekte post-fader, tempo-synchron · Offen: Reverb, und ob vier reichen | M | 1 |
| 8 | Stems | Beim Import vorberechnet, Stem-Deck-Modus | L | 6 |
| ~~9~~ | ~~Mitschnitt~~ ✅ | Summe als WAV, hinter dem Begrenzer, verlorene Frames werden gezählt · Offen: FLAC | S | 1 |
| 10 | MIDI-Controller | Generic-MIDI-Mapping, speicherbar | M | 3 |

Für Phase 10 ist Mixxx' Mapping-Verzeichnis die beste verfügbare Dokumentation
der Geräte-Landschaft — als Nachschlagewerk, nicht als Vorlage zum Kopieren.
Warum das ein Unterschied ist, steht in [MIXXX.md](MIXXX.md).

Phase 10 ist seit dem Steuerraum deutlich kleiner geworden: Ein Mapping ist
jetzt eine Tabelle von MIDI-Nachricht auf Control-Namen, und den normierten
Schreibweg (`setn`, 0..1) gibt es bereits. Siehe
[STEUERUNG.md](STEUERUNG.md).

Phase 1 und 2 sind unabhängig und können parallel laufen — die Analyse braucht
die Engine nicht.

Ab Phase 6 ist die Software als DJ-Werkzeug benutzbar. 7 bis 10 machen sie gut.

**Wenige gute Effekte statt vierzig.** Traktor hat über 40; die meisten davon
benutzt niemand. Gebaut sind Delay, Gater, Flanger und Crusher — der Filter
sitzt ohnehin schon fest im Kanalzug. **Reverb fehlt bewusst:** Ein schlechter
Hall ist schlimmer als keiner, und ein guter ist ein eigenes Stück Arbeit.

Alle vier liegen **hinter dem Fader**. Das ist der Punkt, an dem Mixer-FX sich
von Insert-Effekten unterscheiden: Zieht man den Fader zu, während ein Delay
klingt, soll die Fahne ausklingen statt abzureißen. Der Mixer muss dafür einen
stummen Kanal weiterrechnen, solange sein Effekt noch klingt — sonst wäre die
Anordnung nur auf dem Papier richtig.

## Was inzwischen steht

Neun von zehn Phasen sind gebaut. Die Software lässt sich bedienen: Tracks
aus der Sammlung auf die Decks legen, Wellenform und Beatgrid sehen, nach
Tempo und Tonart aussuchen, mischen. Was fehlt, sind Stems und MIDI.

Die Tonarterkennung ist selbst gebaut — libKeyFinder und der QM Key Detector
stehen unter GPL und hätten den Weg zu VibeMind zugemacht. Sie sagt bewusst
**nichts**, wenn ein Track nur aus Bass und Drums besteht: Dort steht das
Tongeschlecht nicht im Signal, und ein geratenes Dur führt auf dem Camelot-Rad
zum Fehlgriff. Die Schwellen sind an synthetischem Material gemessen und
gegen eine echte Sammlung nie geprüft.

| Crate | Inhalt |
| --- | --- |
| `audio-core` | Deck: Dekodierung, WSOLA, Beatgrid, Hot Cues, Loops |
| `audio-engine` | Mixer: Kanalzüge, EQ, Filter, Effekte, Crossfader, Cue-Bus, Begrenzer, AUX, Sync, Mitschnitt, Kommandoschlange, Geräteausgabe |
| `analysis` | Tempo, Beatgrid, Tonart, Wellenform-Spitzen, Sidecar-Cache |
| `library` | SQLite-Sammlung, Suche nach Text, Tempo und Tonart, Playlists, Traktor-Import |
| `musik-cli` | Deck fahren, analysieren, Mix rendern, Sammlung verwalten |
| `control` | Steuerraum: benannte Controls, Katalog, Zeilenprotokoll, Socket |
| `musik-app` | Oberfläche: zwei Decks, Mixer mit AUX, Plattenkiste |

## Offene Entscheidungen

**Streaming von Platte statt Vollentschlüsselung** (spätestens Phase 8). Vier
Decks à 100 MB gehen noch; vier Decks mit je vier Stems nicht mehr. Dann wird
aus dem Laden ein Vorpuffer plus Nachladen im Hintergrund. Mixxx hat genau das
gelöst und beschreibt es im Wiki — lesen, bevor wir es selbst entwerfen, siehe
[MIXXX.md](MIXXX.md#4-streaming-von-platte).

**Kalibrierung der Tempo-Schwelle.** Die Grenze, ab der ein erkanntes Tempo als
Aussage gilt statt als Rauschen, ist inzwischen ein z-Wert statt eines
Verhältnisses zum Mittelwert — das alte Maß hat dichtes Material
fälschlich abgewiesen, weil dessen durchgehende Energie den Sockel der
Autokorrelation anhebt (Details in `crates/analysis/src/tempo.rs`). Sie trennt
jetzt Klick-Track, dichten Loop und Dauerton sauber, ist gegen echte Musik aber
weiterhin nicht geprüft; das geht erst mit einer Sammlung auf der Platte.

## Entschiedene Punkte

**UI-Framework: egui/eframe** (MIT/Apache-2.0, GPU-beschleunigt über wgpu).

Eine DJ-Oberfläche zeichnet ohnehin jedes Bild neu — laufende Waveforms, bewegte
Fader, Pegelanzeigen. Immediate-Mode ist dafür eher Vorteil als Nachteil, und
die Waveform wird selbst gemalt statt aus Widgets zusammengesetzt.

Zwei Alternativen wurden verworfen:

- **Tauri** wäre näher an vorhandener TypeScript-Erfahrung, aber die
  Zustandskopplung zwischen UI und Audio ist eng und hochfrequent. Das über IPC
  zu führen ist Aufwand, den egui nicht hat. Dazu hängt das Rendering an der
  System-Webview, und unter Linux ist WebKitGTK bei canvas-lastigen Oberflächen
  die schwächste der Engines.
- **Electron** bringt gebündeltes Chromium und damit identisches Rendering
  überall, verlangt aber ein natives N-API-Addon zum Rust-Kern samt Prebuilds
  für drei Plattformen — bei ~200 MB Bundle. Cross-Plattform ist dabei kein
  Argument: egui und Tauri bauen genauso für Windows, macOS und Linux.

**Streaming von Platte statt Vollentschlüsselung** (spätestens Phase 8). Vier
Decks à 100 MB gehen noch; vier Decks mit je vier Stems nicht mehr. Dann wird
aus dem Laden ein Vorpuffer plus Nachladen im Hintergrund. Mixxx hat genau das
gelöst und beschreibt es im Wiki — lesen, bevor wir es selbst entwerfen, siehe
[MIXXX.md](MIXXX.md#4-streaming-von-platte).

## Risiken

| Risiko | Auswirkung | Umgang |
| --- | --- | --- |
| Cue-Ausgang braucht bestimmte Hardware | Phase 1 nicht abnehmbar | Vor Phase 1 klären, nicht währenddessen |
| Eigene WSOLA klingt bei ±8 % nicht gut genug | Kernfunktion unbrauchbar | Rubber Band per FFI — steht offen, siehe [BAUSTEINE.md](BAUSTEINE.md) |
| Beatgrid-Erkennung bei nicht-geradem Material | Sync unbrauchbar für Teile der Sammlung | Manuelle Grid-Korrektur einplanen, nicht als Notlösung, sondern als Feature |
| Stem-Separation zu langsam | Phase 8 zieht sich | Beim Import vorberechnen, Fortschritt anzeigen, nie im Abspielpfad |
| UI schafft keine 60 fps mit Waveforms | Fühlt sich träge an | Peaks vorberechnet halten, nie im Zeichenpfad rechnen |

## Nächster konkreter Schritt

Vor Phase 1 steht eine Hörprobe: die vorhandene WSOLA mit einem echten Track bei
±8 % beurteilen. Fällt sie durch, ändert das die Reihenfolge — dann kommt der
Stretcher-Tausch vor dem Mehrdeck-Umbau, weil sonst auf einem wackligen
Fundament weitergebaut wird.
