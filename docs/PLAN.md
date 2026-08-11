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
  audio-core/    steht — Dekodierung, WSOLA, ein Deck, CPAL-Ausgabe
  audio-engine/  NEU — Mehrdeck-Graph, Mixer, Cue-Bus, Effekte, Master
  analysis/      NEU — BPM, Beatgrid, Tonart, Waveform-Peaks (offline)
  library/       NEU — SQLite, Metadaten, Playlists, Traktor-Import
  sources/       NEU — Trackquellen: Datei, später Generierung
  musik-app/     NEU — UI
  musik-cli/     steht — bleibt als Testwerkzeug
```

`audio-core` behält seine Rolle als Deck-Innenleben; `audio-engine` ist alles
darüber. Die Trennung lohnt sich, weil `audio-engine` die Echtzeitregeln
durchsetzt und `analysis`/`library` sie bewusst nicht kennen müssen.

### Signalkette pro Kanal

```
Deck ─► Trim ─► EQ (3-Band) ─► Filter (HP/LP) ─► Kanalfader ─┬─► Crossfader ─► Master-Summe ─► Limiter ─► Ausgang 1/2
                                                             │
                                                             └─► Cue-Bus ──────────────────────────────► Ausgang 3/4
```

Zwei Details aus der Traktor-Referenz, die man leicht falsch baut:

- **Mixer-FX liegen post-fader.** Zieht man den Fader zu, klingt der Effekt aus
  statt abzureißen. Das ist der Unterschied zwischen einem weichen und einem
  ruckeligen Übergang.
- **Der Cue-Abgriff liegt vor dem Crossfader.** Sonst hört man im Kopfhörer
  nichts, wenn der Kanal ausgeblendet ist — also genau dann, wenn man ihn
  braucht.

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
| 1 | Mehrdeck-Engine, Mixer, Cue-Bus | Zwei Tracks laufen gleichzeitig, Crossfader blendet, Cue liegt hörbar getrennt auf 3/4 | M | 4-Kanal-Interface |
| 2 | Analyse-Pipeline | BPM und Grid reproduzierbar als Sidecar, Peaks vorhanden | M | — |
| 3 | UI-Grundgerüst | Zwei Decks mit Waveform, Mixer bedienbar, stabile 60 fps | L | 1, 2 |
| 4 | Sync und Beatmatching | Zwei Tracks bleiben über 5 Minuten im Takt, Phase korrekt | M | 2 |
| 5 | Hot Cues und Loops | 8 Cues pro Deck, Loop in/out, beat-quantisiert | M | 4 |
| 6 | Library | Import, Suche, Playlists, Traktor-`collection.nml` | L | 2 |
| 7 | Effekte | 6–8 gute Effekte, Mixer-FX post-fader | M | 1 |
| 8 | Stems | Beim Import vorberechnet, Stem-Deck-Modus | L | 6 |
| 9 | Mitschnitt | Master-Bus als WAV/FLAC, ohne Aussetzer | S | 1 |
| 10 | MIDI-Controller | Generic-MIDI-Mapping, speicherbar | M | 3 |

Phase 1 und 2 sind unabhängig und können parallel laufen — die Analyse braucht
die Engine nicht.

Ab Phase 6 ist die Software als DJ-Werkzeug benutzbar. 7 bis 10 machen sie gut.

**Wenige gute Effekte statt vierzig.** Traktor hat über 40; die meisten davon
benutzt niemand. Sinnvoll sind Filter, Delay, Reverb, Beatmasher/Gater, Flanger
und ein Bitcrusher — richtig gebaut und mit vernünftigen Regelwegen.

## Offene Entscheidungen

**UI-Framework** (Phase 3). Empfehlung: **egui/eframe** (MIT/Apache-2.0,
GPU-beschleunigt). Passt zu einer DJ-Oberfläche, weil die ohnehin jedes Bild neu
zeichnet — laufende Waveforms, bewegte Fader, Pegelanzeigen. Immediate-Mode ist
für so etwas eher Vorteil als Nachteil, und die Waveform wird selbst gemalt statt
aus Widgets zusammengesetzt.

Die Alternative wäre Tauri mit TypeScript-Frontend. Näher an vorhandener
Erfahrung, aber die Zustandskopplung zwischen UI und Audio ist eng und
hochfrequent — das über IPC zu führen ist Aufwand, den egui nicht hat.

**Streaming von Platte statt Vollentschlüsselung** (spätestens Phase 8). Vier
Decks à 100 MB gehen noch; vier Decks mit je vier Stems nicht mehr. Dann wird
aus dem Laden ein Vorpuffer plus Nachladen im Hintergrund.

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
