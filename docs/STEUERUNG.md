# Steuerung von außen

Die Anlage lässt sich fernsteuern, während sie läuft. Ein Socket, ein
Zeilenprotokoll, keine Bibliothek nötig:

```sh
$ nc -U "$XDG_RUNTIME_DIR/musik.sock"
set deck1.play 1
ok deck1.play 1
```

Das ist der Punkt, an dem sich dieses Projekt von den fertigen Alternativen
unterscheidet. Mixxx hat ein sehr ähnliches Control-System im Inneren — dort
laufen Tastatur, MIDI, HID und Oberfläche über dieselben benannten Werte —
aber es ist prozessintern. Nach außen spricht Mixxx OSC nur *ausgehend*:
Es sendet Zustand, es nimmt keine Befehle entgegen. Siehe
[MIXXX.md](MIXXX.md#was-mixxx-nicht-löst).

Ohne diese Schnittstelle gäbe es die Agenten-Schicht aus
[AGENTEN.md](AGENTEN.md) nicht, und der Anschluss an
[VibeMind](VIBEMIND.md) hätte nichts, woran er andocken könnte.

## Der Steuerraum

Jedes Control heißt `gruppe.element`:

| Gruppe | Was |
| --- | --- |
| `deck1`, `deck2`, … | Abspieler: Transport, Tempo, Hot Cues, Schleifen |
| `channel1`, `channel2`, … | Züge am Mischpult: Trim, EQ, Filter, Fader, Cue |
| `master` | Crossfader, Summe, Kopfhörer |

Deck und Kanal sind absichtlich getrennt. Bei Mixxx fallen sie in `[Channel1]`
zusammen, was so lange gutgeht, bis ein Kanal etwas führt, das kein Deck ist —
bei uns der AUX-Eingang, der einen Zug hat und kein Deck.

## Befehle

| Befehl | Wirkung |
| --- | --- |
| `list [praefix]` | Controls aufzählen, gefiltert nach Namensanfang |
| `get <control>` | Wert lesen |
| `set <control> <wert>` | Wert setzen; `-` leert (Hot Cues) |
| `setn <control> <0..1>` | normiert setzen — für MIDI |
| `help` | Übersicht |

Antworten beginnen mit `control`, `value`, `ok` oder `err`. Ein `err` ist immer
als solches erkennbar; nichts scheitert stumm.

## Der Steuerraum beschreibt sich selbst

Das ist der zweite Unterschied. `list` liefert nicht bloß Namen, sondern
Typ, Bereich, Einheit, Schreibbarkeit und eine Bedeutung:

```text
$ echo 'list deck1.tempo' | nc -U -q1 "$XDG_RUNTIME_DIR/musik.sock"
control deck1.tempo zahl 0.92..1.08 faktor rw Tempo-Regler; 1.0 ist die Originalgeschwindigkeit
ok 1 Controls
```

Wer das gelesen hat, braucht kein Handbuch: Er weiß, dass es eine Zahl ist,
zwischen 0,92 und 1,08 liegt, ein Faktor ist, sich schreiben lässt und was sie
bedeutet. Bei Mixxx steht all das in der Dokumentation, und ein laufendes
Programm lässt sich nicht fragen.

Der praktische Gewinn ist doppelt:

- **Für Agenten.** Die MCP-Werkzeugbeschreibungen lassen sich aus dem Katalog
  erzeugen, statt sie von Hand doppelt zu pflegen — zwei Beschreibungen, die
  auseinanderlaufen, gibt es dann nicht.
- **Für die eigene Oberfläche.** Die Schieberegler im Mixer holen sich Bereich
  und Tooltip aus demselben Katalog. Ein neues Control ist ohne eine Zeile in
  der Oberfläche bedienbar, und der Hilfetext, den ein Agent sieht, ist
  derselbe, der unter der Maus erscheint.

## Werte sind typisiert

Bei Mixxx ist jedes Control ein `double`; `play` ist 0.0 oder 1.0, eine
Zuweisung eine durchnummerierte Zahl. Das ist einfach zu bauen und schwer zu
benutzen — ein Tippfehler wird zu einem gültigen Wert statt zu einem Fehler.

Hier gibt es Schalter, Zahlen mit Bereich, Auswahlen mit Namen und Text:

```text
set channel1.assign thru      → ok channel1.assign thru
set channel1.assign mitte     → err channel1.assign kennt nur: a, b, thru
set deck1.duration 5          → err deck1.duration lässt sich nur lesen
```

Zwei bewusste Ausnahmen von der Strenge:

- **Werte außerhalb des Bereichs werden begrenzt, nicht abgelehnt.** Ein
  MIDI-Regler auf Anschlag meint das Maximum, keinen Fehler.
- **Die Bestätigung nennt den Wert, der wirklich angekommen ist** — nicht den
  gesendeten. `set channel1.fader 9` antwortet `ok channel1.fader 1`.

## Ein Beispiel, das wirklich lief

Zwei Decks von außen ineinandergefahren, während die Oberfläche zusah:

```sh
set deck1.play 1
set deck2.play 1
set channel1.fader 0.9
set channel2.fader 0.55
setn channel2.eq_low 0.0        # Bass-Kill über den normierten Weg
set channel2.filter 0.45
set channel1.cue 1
set master.crossfader -0.35
set deck1.cue1 4.0
set deck1.loop_beats 4
set deck2.tempo 1.032           # 124 × 1,032 = 127,97 — auf Deck A gezogen
```

Danach zeigte die Oberfläche 127,99 gegen 127,98 BPM, den gesetzten Loop, die
Hot-Cue-Marken in der Wellenform und jeden bewegten Regler an der richtigen
Stelle. Screenshot: [`bilder/fernsteuerung.png`](bilder/fernsteuerung.png).

## Warum ein Unix-Socket und kein TCP-Port

Eine Sicherheitsentscheidung, keine Bequemlichkeit. Wer hier hineinschreibt,
steuert die Anlage. Ein offener TCP-Port täte das für jeden im selben Netz —
auf einer Bühne mit fremdem WLAN ist das keine theoretische Sorge. Ein
Unix-Socket erbt die Rechte des Dateisystems: Wer die Datei nicht öffnen darf,
kommt nicht hinein.

Der Standardpfad liegt aus demselben Grund in `$XDG_RUNTIME_DIR` und nicht in
`/tmp` — dort könnte jeder andere Benutzer des Rechners mitsteuern. Mit
`--socket <pfad>` lässt er sich verlegen.

Es gibt **keine Authentifizierung**. Sie wäre bei Dateisystemrechten
verdoppelt. Sollte die Steuerung je über das Netz gehen, ist das die Stelle, an
der sie nachzurüsten ist — vorher nicht.

## Echtzeit bleibt unangetastet

Das Pult schreibt nie in den Audio-Callback. Es schickt Kommandos in dieselbe
lock-freie Schlange, die auch die Oberfläche benutzt, und liest aus Atomics
und aus dem eigenen Spiegel. Der Mutex um das Pult wird nur von Bedienern
genommen — Oberfläche, Socket, später MIDI. Der Audio-Thread sieht ihn nie.

## Was noch fehlt

- **Abonnieren.** Wer den Zustand verfolgen will, muss zurzeit pollen. Ein
  `sub deck1.position` mit Meldungen bei Änderung ist der nächste Schritt und
  Voraussetzung für Controller mit Motorfadern und Displays.
- **Laden von außen.** `deck1.load <pfad>` fehlt. Dekodieren und Analysieren
  dauern; das braucht einen asynchronen Auftrag statt einer Antwortzeile.
- **Windows.** Unix-Sockets gibt es dort nicht; eine Named Pipe wäre die
  Entsprechung.
- **MIDI.** Der normierte Weg (`setn`) ist da, der Übersetzer von MIDI-CC auf
  Control-Namen noch nicht. Das ist Phase 10 — und mit dem Steuerraum im Rücken
  fast nur noch eine Tabelle.
