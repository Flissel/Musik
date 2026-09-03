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
| `do <control> [arg]` | Aktion auslösen |
| `sub <control>…` | Änderungen melden lassen, statt zu fragen |
| `unsub [control]…` | abbestellen; ohne Argument alles |
| `help` | Übersicht |

Antworten beginnen mit `control`, `value`, `ok` oder `err`. Ein `err` ist immer
als solches erkennbar; nichts scheitert stumm.

## Aktionen sind keine Werte

`sync`, `load`, einen Hot Cue anspringen — das sind Auslöser, keine
Reglerstellungen. Mixxx modelliert so etwas als Control, in das man 1.0
schreibt (`[Channel1],cue_gotoandplay`). Das funktioniert, ist aber dieselbe
Schwäche wie beim `double`: Ein Auslöser hat keinen Zustand, den man lesen
könnte, und `get` darauf müsste etwas erfinden.

Hier haben sie ein eigenes Verb:

```text
do deck2.sync                → sync deck2 auf deck1 tempo 1.01570 phase -0.1178
do deck1.load /musik/a.wav   → load deck1 angenommen
do deck2.jump_cue 1
do deck1.beatjump -4
do master.search techno
do master.search_mixable 128
do master.search_harmonic 8A
get deck1.sync               → err deck1.sync ist eine Aktion — mit 'do' auslösen
set deck1.sync 1             → err deck1.sync ist eine Aktion — mit 'do' auslösen
do deck1.play                → err deck1.play ist keine Aktion — mit 'set' setzen
```

**`sync` ist Tempo *und* Phase**, in einem Befehl. Ohne das müsste ein Agent
beide BPM lesen, den Quotienten bilden, das Tempo setzen und die Phase selbst
ausrechnen — und die Phase vergisst man dabei, weil das Ergebnis auch ohne sie
plausibel aussieht.

**`load` arbeitet im Hintergrund.** Dekodieren und Analysieren dauern Sekunden,
und das Pult liegt währenddessen unter einem Mutex, an dem die Oberfläche
hängt. Der Befehl meldet deshalb *Annahme*, nicht Erledigung; der Fortschritt
steht in `deckN.load_status`. Ein Auftrag, der abgelehnt wird — falscher Pfad
etwa — lässt den Status unberührt, statt ein Laden vorzutäuschen, das nie
begonnen hat.

## Zuhören statt fragen

```text
sub deck1.finished deck1.position
ok sub 2 neu, 2 gesamt
value deck1.finished 0
value deck1.position 12.354000
value deck1.position 12.404000
…
value deck1.finished 1
```

Das erste, was ein Abo meldet, ist der Ist-Zustand — sonst müsste man nach dem
Abonnieren noch einmal `get` sagen. Danach kommt nur, was sich geändert hat.

Ehrlich gesagt: Das Pult meldet nichts von sich aus, der Server vergleicht alle
50 ms. Für Werte, die im Audio-Thread laufen, wäre eine echte Benachrichtigung
auch sinnlos — die Position ändert sich mit jedem Sample, die will niemand
vollständig. Der Gewinn liegt darin, dass der Bediener das nicht selbst bauen
muss, und dass `deckN.finished` ohne Dauerabfrage ankommt.

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

## Ein vollständiger Übergang, der wirklich lief

Kein Mausklick, keine Kenntnis der Oberfläche — nur Control-Namen:

```sh
do master.search Alpen                  # was gibt es?
do deck1.load "…/Alpenglühen.wav"       # auflegen
do master.search_mixable 128            # was passt vom Tempo?
get deck1.key_camelot                   # → 8A
do master.search_harmonic 8A            # was passt von der Tonart?
do deck2.load "…/Nachtschicht.wav"
set deck1.play 1
set channel1.fader 0.9
do deck2.sync                           # Tempo UND Phase, in einem Befehl
set deck2.cue1 8.0
do deck2.jump_cue 1
set deck2.play 1
setn channel2.eq_low 0                  # Bass raus …
set channel2.fader 0.9
set master.crossfader 0.25
setn channel1.eq_low 0                  # … und drüben rein: Bass-Swap
setn channel2.eq_low 0.5
do deck1.beatjump -4
sub deck1.finished deck1.position       # aufs Ende horchen
```

Die Antwort auf `do deck2.sync` war

```text
sync deck2 auf deck1 tempo 1.01570 phase -0.1178
```

und danach standen beide Decks auf **127,986 BPM** — 126,008 × 1,0157. Die
Oberfläche zeigte das Ergebnis unmittelbar: geladene Titel, Tempo +1,57 %,
die Hot-Cue-Marke in der Wellenform, gekillten Bass, Crossfader auf +0,25.

![Ein Übergang, komplett von außen](bilder/agent-uebergang.png)

Für eine ältere, einfachere Aufnahme ohne Laden und Sync siehe
[`bilder/fernsteuerung.png`](bilder/fernsteuerung.png).

## Warum ein Unix-Socket und kein TCP-Port

Eine Sicherheitsentscheidung, keine Bequemlichkeit. Wer hier hineinschreibt,
steuert die Anlage. Ein offener TCP-Port täte das für jeden im selben Netz —
auf einer Bühne mit fremdem WLAN ist das keine theoretische Sorge. Ein
Unix-Socket erbt die Rechte des Dateisystems: Wer die Datei nicht öffnen darf,
kommt nicht hinein.

Der Standardpfad liegt aus demselben Grund in `$XDG_RUNTIME_DIR` und nicht in
`/tmp` — dort könnte jeder andere Benutzer des Rechners mitsteuern. Mit
`--socket <pfad>` lässt er sich verlegen.

Über den Socket gibt es **keine Authentifizierung**. Sie wäre bei
Dateisystemrechten verdoppelt.

### Wo es keinen Socket gibt

Auf Windows gibt es keine Unix-Sockets, und dort lief die Anlage bis vor Kurzem
**ohne jede Steuerung** — Oberfläche, Decks und Mixer ja, Fernsteuerung nein,
und damit auch kein MCP und keine Agenten. Also genau der Teil, um
dessentwillen das Projekt gebaut wird.

`musik-app --tcp` lauscht deshalb auf der Rückschleife. Der Absatz oben hatte
vorausgesagt, wo dann nachzurüsten ist, und genau das ist geschehen:

```text
$ musik-app --tcp
Steuerung: 127.0.0.1:7657
Schlüssel: 50270126063f57dd96e73b655ce56d5a

$ nc 127.0.0.1 7657
auth 50270126063f57dd96e73b655ce56d5a
ok angemeldet
set deck1.play 1
```

Zwei Dinge sind dabei enger gebaut als beim Socket, weil TCP von sich aus
weiter offen steht:

- **Nur die Rückschleife.** Gebunden wird ausschließlich an `127.0.0.1` oder
  `::1`; eine andere Adresse wird abgelehnt, nicht stillschweigend übernommen.
  Ein Tippfehler soll die Anlage nicht ins WLAN stellen. Geprüft wird **vor**
  dem Binden — danach läge zwischen Öffnen und Schließen genau das Fenster, das
  man nicht haben will.
- **Ein Schlüssel je Start.** Die erste Zeile muss `auth <schlüssel>` sein,
  sonst wird nichts ausgeführt, auch nichts Lesendes: Wer nicht hereindarf, soll
  nicht erfahren, was gerade läuft. Der Schlüssel steht nur im Fenster der
  Anwendung — in eine Datei geschrieben läge er dort, wo ihn jeder findet.

**Der Schlüssel ist kein Zierrat.** Die Rückschleife ist nicht privat: Jedes
Programm auf demselben Rechner erreicht sie, eine Webseite im Browser
eingeschlossen. Ein `fetch` an `http://127.0.0.1:7657` schickt Zeilen, die
dieses Protokoll liest. Nachgestellt mit einer echten HTTP-Anfrage samt Rumpf
`set master.crossfader 1`: Jede Zeile — Anfragezeile, Kopfzeilen und Rumpf —
bekam `err nicht angemeldet`, und der Crossfader stand hinterher unverändert
auf −0,5.

Wo es einen Unix-Socket gibt, bleibt er die erste Wahl: Dort erledigt das
Dateisystem, wofür hier ein Schlüssel nötig ist.

## Echtzeit bleibt unangetastet

Das Pult schreibt nie in den Audio-Callback. Es schickt Kommandos in dieselbe
lock-freie Schlange, die auch die Oberfläche benutzt, und liest aus Atomics
und aus dem eigenen Spiegel. Der Mutex um das Pult wird nur von Bedienern
genommen — Oberfläche, Socket, später MIDI. Der Audio-Thread sieht ihn nie.

## Die Probe aufs Exempel: die Effekte

Als die Effekte dazukamen, bekam die Oberfläche **keine einzige Zeile** für
sie. Der Kanalzug holt sich seine Regler aus dem Katalog — Reihenfolge,
Bereich, Beschriftung, Hilfetext. Vier neue Einträge im Katalog, und das
FX-Feld stand da, samt Auswahlfeld mit den richtigen Namen:

```text
$ echo 'list channel1.fx' | nc -U -q1 "$XDG_RUNTIME_DIR/musik.sock"
control channel1.fx auswahl off|delay|gater|flanger|crush - rw Effekt hinter dem Fader; …
```

Ein Nebeneffekt davon ist, dass die Katalogreihenfolge jetzt die Anordnung am
Gerät ist: Höhen oben, Bässe unten. Beim ersten Versuch stand der EQ
verkehrtherum, weil im Katalog `eq_low` vor `eq_high` stand — im Zweifel greift
man beim Auflegen daneben, und deshalb ist die Reihenfolge dort jetzt
festgehalten und begründet.

![Effekte, von außen gesetzt](bilder/effekte.png)

## Mitschnitt

```text
do master.record ~/sets/2026-08-13.wav
record läuft nach ~/sets/2026-08-13.wav
mitschrift ~/sets/2026-08-13.mitschrift
get master.record_seconds     → value master.record_seconds 6.784000
get master.record_dropped     → value master.record_dropped 0
do master.record_stop         → record ~/sets/2026-08-13.wav · 6.8 s
                                mitschrift ~/sets/2026-08-13.mitschrift
```

Aufgenommen wird die Summe **hinter dem Begrenzer** — also das, was auf die
Anlage geht, und nicht das, was vorher da war.

`record_dropped` ist kein Beiwerk. Der Audio-Thread darf nicht auf die Platte
schreiben; er legt in einen Ringpuffer, ein eigener Thread leert ihn. Kommt der
nicht hinterher, gehen Frames verloren — blockieren wäre schlimmer. Verloren
heißt hier aber nicht verschwiegen: Der Zähler ist abfragbar, und `record_stop`
sagt es von sich aus dazu. Ein Mitschnitt mit Lücken, der aussieht wie einer
ohne, wäre das schlechteste Ergebnis von allen.

## Die Mitschrift

Neben dem Mitschnitt entsteht eine zweite Datei, gleicher Name, Endung
`.mitschrift`. Sie wird nicht einzeln eingeschaltet — der Klang ohne die
Absicht ist die Hälfte der Geschichte.

```text
# musik-mitschrift 1
# mitschnitt /pfad/set.wav
# rate 48000
962560 20.053 deck1=41.187/16 deck2=-0.998/16~ > in phrase set deck2.play 1
1116160 23.253 deck1=48.012/16 deck2=-0.998/16~ > ramp master.crossfader 1 32
1837056 38.272 deck1=80.049/16 deck2=31.039/16 < plan 3 fertig master.crossfader 1.0000
```

Je Zeile: **Frame im Mitschnitt**, Sekunden zur Bequemlichkeit, dann für jedes
Deck mit Beatgrid `deck<N>=<beat>/<phrase>` — ein angehängtes `~` heißt, das
Deck stand. `>` ist hereingekommen, `<` ist hinausgegangen.

Drinsteht, was **verändert**: `set`, `setn`, `do`, `ramp`, `in`, `when`,
`cancel`, dazu die Meldungen des Zeitplans. `get`, `list`, `plan` und `help`
fragen nur und stünden sonst in jeder zweiten Zeile.

**Wozu.** Der Griff an den Fader steht nicht im Klang. Am Anfang einer langen
Blende ist der eingehende Track per Konstruktion unhörbar; wer den Beginn
nachträglich aus dem Mitschnitt schätzt, liegt an einem gemessenen Beispiel
3,7 Sekunden daneben — über 24 gefahrene Sets im Median 2,0 Sekunden, bei
langen Blenden bis 7,6, und über 64 Beats findet er den Übergang gar nicht
mehr. Dieselbe Lücke bei der Phrasenlage: Der Anker eines
nachträglich geschätzten Rasters ist irgendein starker Schlag, nicht die Eins.
Beides weiß die Anlage im Moment des Geschehens genau.

Gelesen wird sie von `musik-kritik`, der sie von selbst neben dem Mitschnitt
findet. Maßgeblich ist immer der **Frame**; die Sekunden in der Zeile sind für
Menschen da und werden beim Lesen aus der Rate neu gerechnet.

## Antworten lesen

Wer einen eigenen Bediener schreibt, braucht eine Abbruchregel. Eine Antwort
endet mit einer Zeile, die mit `ok`, `err` oder `value` beginnt:

| Befehl | Antwort |
| --- | --- |
| `get` | genau eine `value`-Zeile |
| `list` | eine `control`-Zeile je Treffer, dann `ok N Controls` |
| `set`, `setn` | eine `ok`- oder `err`-Zeile |
| `do` | keine bis mehrere Infozeilen, dann `ok` oder `err` |
| `sub` | `ok`, danach `value`-Zeilen, wann immer sich etwas ändert |

Ein Abo ist der einzige Fall, in dem ungefragt etwas hereinkommt. Wer
abonniert, sollte deshalb getrennt lesen und schreiben.

## Anschluss für Agenten: MCP

Für Agenten liegt eine Brücke bei — [`mcp/musik/`](../mcp/musik/README.md), ein
MCP-Server, der dieses Protokoll in Werkzeuge übersetzt. Vierzehn Werkzeuge statt
zweihundert: `musik_set` und `musik_do` erreichen alles, `musik_status` und
`musik_search` sparen die häufigsten Wege, `musik_ramp`, `musik_schedule` und
`musik_cancel` reichen den Zeitplan durch, `musik_queue`, `musik_queue_add` und
`musik_queue_next` die Liste.

Der Gewinn aus dem selbstbeschreibenden Katalog wird dort eingelöst: Die
Beschreibungen von `musik_set` und `musik_do` **erzeugt der Server beim Start
aus dem laufenden Programm**. Was ein Agent liest, ist dasselbe, was im Katalog
steht und in der Oberfläche als Tooltip erscheint.

Beim Bauen fiel dabei auf, dass `list` das Argument einer Aktion nicht
mitgab — es stand im Katalog, verließ aber den Prozess nie. Jetzt steht es in
der Bereichsspalte, und `deck1.load` sagt von sich aus, dass es einen Pfad
will.

## Zeit: Übergänge statt Reglerstellungen

Der Teil, der aus dem Steuerraum ein Werkzeug für ein **Team von Agenten**
macht. Ein Übergang ist keine Folge von Reglerstellungen, sondern eine Bewegung
über Takte. Wer das von außen nachbaut, müsste in einer engen Schleife
`beat_phase` pollen und dazwischen schlafen — über eine Leitung, deren Timing
dem Scheduler des Betriebssystems ausgeliefert ist. Das eiert hörbar, und der
Agent ist die ganze Zeit blockiert.

```text
set channel2.fader 0.0
ramp channel1.eq_low 0.0 8            # Bass raus über 8 Beats
in 8 ramp channel2.fader 0.9 16       # danach Deck B rein über 16
in 8 ramp master.crossfader 1.0 16
plan                                  # was vorgemerkt ist
cancel 2                              # einen Auftrag zurücknehmen
```

Drei Zeilen, dann kann der Agent auflegen. Gelaufen ist es so:

| nach | `channel1.eq_low` | `channel2.fader` | `master.crossfader` |
| --- | --- | --- | --- |
| 2 s | 0,47 | 0 | 0 |
| 5 s | 0 | 0,13 | 0,15 |
| 9 s | 0 | 0,90 | 1,00 |

**Gerechnet wird in Beats, nicht in Sekunden.** Ein Plan in Sekunden geht
schief, sobald jemand am Tempo dreht; einer in Beats bleibt musikalisch
richtig. Steht das Deck, steht auch der Plan — was gestoppt ist, hat keine
Takte. Welches Deck den Takt vorgibt, ergibt sich von selbst: Ein Kanalzug erbt
ihn von dem Deck, das auf ihm liegt, ein Deck ist sein eigener Taktgeber, und
die Summe nimmt das erste Deck mit Grid. Mit einem vierten Wort lässt sich das
überschreiben (`ramp master.crossfader 1.0 16 deck2`).

### Der Mensch gewinnt

**Eine Rampe gibt auf, sobald jemand anders denselben Regler anfasst.**

```text
> ramp channel1.fader 0.0 64
< ok plan 5 ramp channel1.fader nach 0 über 64 Beats
  … nach 3 s steht der Fader bei 0.909699 und wandert
> set channel1.fader 0.75
  … nach 3 s steht er immer noch bei 0.75, und der Plan ist leer
```

Ohne das wäre die Automatik stärker als der Griff daneben: Man zieht den Fader
zu, und einen Wimpernschlag später steht er wieder offen. Geprüft wird nicht
über eine Kennung, sondern über den Wert selbst — steht dort etwas anderes, als
die Rampe zuletzt geschrieben hat, war jemand anders am Werk. Das gilt für
einen Menschen an der Oberfläche genauso wie für einen zweiten Agenten.

Damit ist `plan` zugleich das **gemeinsame Blatt**: Wer mitliest, sieht, was die
anderen vorhaben, statt es aus Reglerbewegungen zu erraten. Und weil der Mensch
an der Oberfläche derselbe Mitbediener ist, steht es dort auch — in der
Regie-Spalte rechts, mit Fortschrittsbalken je Rampe. Ohne sie gewönne er jeden
Griff blind: Er sähe einen Fader wandern und wüsste nicht, ob ihn jemand zieht
oder ob er selbst hängengeblieben ist.

![Die Regie-Spalte](bilder/regie.png)

Der Takt liegt bei 5 ms — bei 128 BPM etwa ein Prozent eines Beats. Für eine
Blende unhörbar, für einen harten Schnitt auf die Eins gerade noch vertretbar.
**Sample-genau ist das ausdrücklich nicht**; dafür müssten die Befehle im
Audio-Callback liegen, und dorthin gehört keine Reglerlogik.

### Auf der nächsten Eins

`in` nimmt statt einer Zahl auch `phrase` — die nächste Phrasengrenze des ersten
Decks mit Beatgrid.

```text
in phrase do deck2.sync                    # auf der nächsten Eins
in phrase+16 ramp master.crossfader 1 32   # eine Phrase später, dann blenden
```

**Das ist fast immer das Richtige.** Ein Übergang beginnt auf der Eins einer
Phrase, nicht nach einer runden Zahl Beats. Wer ihn mit `in 32` nachbaut, trifft
irgendwo hin — und der Mix klingt danach, egal wie sauber alles andere sitzt.

`ramp` muss dafür nichts von Phrasen wissen: `in phrase ramp …` setzt die
Bewegung auf die Grenze, weil `in` eine beliebige Protokollzeile trägt. Ein
zweiter Phrasenbegriff im Rampen-Verb wäre eine zweite Stelle zum
Auseinanderlaufen.

Ohne Deck mit Beatgrid hat `phrase` keinen Bezugspunkt und wird abgewiesen,
statt stillschweigend auf null zu fallen.

### Wessen Takte gemeint sind

```text
in deck2 phrase+16 set deck1.loop_active 0
```

Ohne Angabe nimmt `in` das erste Deck mit Beatgrid. Steht ein `deckN` direkt
hinter `in`, ist dessen Takt gemeint — auch wenn der Befehl auf ein anderes Deck
wirkt.

**Gebraucht wird das, sobald ein Deck in einer Schleife läuft.** Dessen Beat
wiederholt sich, und alles, was daran hängt, wiederholt sich mit: Eine Rampe
fängt bei jedem Durchlauf von vorn an, und ein Vorgemerktes hinter dem
Schleifenende kommt nie an — auch nicht die Zeile, die die Schleife wieder
lösen soll.

**Läuft der Track eines Taktgebers aus, bricht sein Plan ab und sagt das:**

```text
plan 17 abgebrochen — deck1 ist durchgelaufen
```

Am laufenden Programm gefunden, beim ersten Übergang, der sich selbst
auslöste: Der ausgehende Track lief mitten im Bass-Swap aus, und danach stand
der Crossfader für immer in der Mitte, mit fünf toten Aufträgen im Plan. Ein
Bediener sieht dort einen Übergang, der läuft, und hört einen, der steht. Genau
gegen diesen Fall gibt es den Griff `schleife`.

Ein bloß **angehaltenes** Deck hält den Plan dagegen nur an — dort wartet er
absichtlich weiter. Wer pausiert, will da weitermachen, wo er aufgehört hat.

**Springt ein Taktgeber zurück, bricht sein Plan ab und sagt das:**

```text
plan 3 abgebrochen — deck1 ist zurückgesprungen (Schleife oder Sprung)
```

Das ist der Fall bei jeder Schleife und bei jedem Sprung im Track. Ihn still
weiterlaufen zu lassen wäre das Schlimmste von allem: Der Bediener sieht einen
Plan, der läuft, und hört etwas anderes. Am laufenden Programm sah man den
Crossfader sieben Mal hin und her fahren, bevor das hier stand.

### Sobald es so weit ist

`in` wartet auf Takte. Die Frage, die beim Auflegen wirklich gestellt wird,
lautet aber anders: **wann ist der Track fast durch?** Dafür gibt es `when`.

```text
when deck1.beats_left < 32 do master.queue_next     # nächsten auflegen
when deck1.beats_left < 16 do deck2.sync
when deck1.beats_left < 16 ramp channel2.fader 0.9 16
plan
  plan 4 wenn deck1.beats_left < 32: do master.queue_next (steht bei 47.20)
```

Und damit das überhaupt fragbar ist, tragen die Decks die Größen, die man sonst
selbst ausrechnet:

| Control | Was |
| --- | --- |
| `deckN.beat` | Der wievielte Beat gerade läuft, vom Grid-Anker gezählt |
| `deckN.beats_left` | Beats bis zum Ende — danach richtet sich der Übergang |
| `deckN.beats_to_phrase` | Beats bis zur nächsten Phrasengrenze |
| `deckN.phrase_beats` | Wie lang eine Phrase ist; 16 als Vorgabe, schreibbar |

**Alles in Grid-Beats, nicht in Sekunden.** Ein Beat ist eine feste Zahl
Quell-Frames; schneller abgespielt vergeht er in weniger Zeit, aber es werden
davon nicht mehr. Genauso rechnen `in` und `ramp` — „noch 32 Beats" und „in 32
Beats" meinen dieselbe Strecke. Zwei Zeitrechnungen im selben Steuerraum wären
eine Falle.

Die Phrasenlänge steht je Deck und nicht global: Sie ist eine Eigenschaft der
Musik, und ein Stück in Achtergruppen kann gleichzeitig mit einem in Sechzehnern
auf den Decks liegen.

Drei Dinge sind an `when` bewusst so:

- **Trifft die Bedingung schon zu, läuft der Befehl sofort.** `when` heißt
  „sobald es so weit ist", nicht „beim nächsten Überschreiten". Wer auf eine
  Flanke warten will, prüft vorher selbst.
- **Ein Control, das keine Zahl ist, wird beim Vormerken abgewiesen.** Ein
  Auftrag auf `deck1.play < 1` würde stumm für immer warten, und das ist
  schlimmer als eine Absage.
- **Ein `when` braucht keinen Taktgeber.** Es hängt an einem Wert, nicht an
  Takten, und läuft deshalb auch dann, wenn gerade kein Deck ein Grid hat.

Ohne dieses Verb müsste ein Bediener `beats_left` abonnieren und bekäme zwanzig
Zahlen je Sekunde, aus denen er die eine Schwelle selbst heraussucht. Über eine
Chat-Schnittstelle ist das nicht nur unbequem, sondern unbezahlbar.

### Was von außen hereinkommt

Ein DJ liest die Fläche. Ein Agent kann das nicht sehen — also muss es jemand
hereingeben: ein Mikrofonpegel, eine Umfrage auf dem Handy, ein Mensch, der im
Chat „wird voller" tippt.

```text
set master.signal1_name Energie auf der Flaeche
set master.signal1 0.2
… zwei Minuten später …
set master.signal1 0.7
get master.signal1_trend      → value master.signal1_trend 1.280000
when master.signal1_trend < -0.3 do master.queue_next
```

**Ein einzelner Wert nützt fast nichts.** „Energie 0,7" beantwortet keine Frage;
„0,7 und seit zwei Minuten fallend" beantwortet sie. Deshalb merkt sich ein
Signal seine jüngste Vergangenheit und rechnet daraus den Trend — dieselbe
Überlegung wie bei `beats_left`: Was ein Bediener sonst bei jedem Blick selbst
ausrechnet, rechnet er irgendwann falsch.

| Control | Was |
| --- | --- |
| `master.signalN` | Der Messwert, −1 bis 1 |
| `master.signalN_name` | Wofür er steht; leer heißt ungenutzt |
| `master.signalN_trend` | Änderung je Minute, aus einer Ausgleichsgeraden |
| `master.signalN_age` | Sekunden seit der letzten Meldung |

Vier feste Plätze mit beschriftbarem Namen, wie ein Kanalzug, den man mit
Klebeband beschriftet. Beliebige Namen bräuchten zur Laufzeit geleakte
Zeichenketten, und dieselbe Entscheidung ist schon bei den Hot Cues so gefallen.

Drei Dinge sind bewusst so:

- **Die Steigung kommt aus einer Ausgleichsgeraden**, nicht aus „letzter minus
  erster". Eine einzelne Fehlmessung am Rand würde daraus sonst eine Behauptung
  machen, die die übrigen Proben nicht hergeben.
- **Aus einer Probe wird kein Trend** — dort steht `-`, nicht 0. Eine Null wäre
  die Aussage „ändert sich nicht", und die hat niemand getroffen. Genauso ist
  ein ungenutztes Signal leer und nicht null.
- **Der Wert bleibt stehen, auch wenn lange nichts kam.** Er ist die letzte
  bekannte Lage, und die verschwindet nicht dadurch, dass niemand mehr misst.
  Wie alt sie ist, sagt `_age`; der Trend fällt nach zwei Minuten von selbst weg.

Damit ist ein Signal ein Control wie jedes andere — `sub`, `when` und `ramp`
greifen darauf, ohne dass dafür eine Zeile geschrieben werden musste. Das ist
der ganze Gewinn eines benannten Steuerraums.

### Über MCP

Dieselben vier Verben, für einen Agenten, der kein Zeilenprotokoll spricht:

| Protokoll | MCP |
| --- | --- |
| `ramp <control> <ziel> <beats> [deck]` | `musik_ramp` |
| `in <beats> ramp …` | `musik_ramp` mit `in_beats` |
| `in <beats> <befehl>` | `musik_schedule` |
| `in phrase[+n] <befehl>` | `musik_schedule` mit `ab_phrase`, `musik_ramp` ebenso |
| `when <control> < <wert> <befehl>` | `musik_when` |
| `set master.signalN …` | `musik_signal` (sucht den Platz selbst) |
| `plan` | Feld `plan` in `musik_status` |
| `cancel [id]` | `musik_cancel` |

Der Plan hängt bewusst an `musik_status` statt an einem eigenen Werkzeug: Wer
eine Momentaufnahme nimmt, bevor er zugreift, soll die Absichten der anderen
sehen, ohne sie extra abfragen zu müssen. Und `musik_cancel` verlangt für
„alles" einen eigenen Schalter — eine vergessene Nummer soll nicht die Arbeit
der anderen leeren.

## Was als Nächstes kommt

Der Zeitplan sagt, **wann** etwas geschieht. Die Liste sagt, **was** als
Nächstes kommt — die zweite Hälfte der Koordination, sobald mehr als einer
auswählt.

```text
do master.queue_add /musik/track.mp3   → queue 1 angehaengt /musik/track.mp3
do master.queue_note 1 mehr Druck nach dem Break
do master.queue                        → queue 1 /musik/track.mp3 mehr Druck nach dem Break
do master.queue_bump 1                 → der als Nächstes
do master.queue_next                   → load deck2 angenommen
                                         queue 1 abgenommen /musik/track.mp3
                                         notiz mehr Druck nach dem Break
```

Sie liegt im Pult, hinter demselben Mutex wie alles andere. Wer abnimmt, nimmt
den Eintrag heraus — ein zweiter, der im selben Moment abnimmt, bekommt den
nächsten und nicht denselben. Zwei Agenten, die ihre Liste je für sich halten,
legen irgendwann beide auf dasselbe Deck.

**Jeder Eintrag trägt eine Notiz**, und das ist der Unterschied zu einer
Playlist. Wer vormerkt, weiß warum; ohne die Notiz muss der Nächste den Grund
aus BPM und Tonart erraten, und bei einem Team von Agenten heißt erraten:
erfinden.

Drei Entscheidungen, die aus dem Mehrbedienerfall kommen:

- **Derselbe Pfad wird nicht zweimal angenommen** — die Antwort nennt die
  Nummer, unter der er schon steht. Zwei, die unabhängig nach 128 BPM in 8A
  suchen, finden denselben Track.
- **Nummern bleiben stehen**, auch wenn davor etwas herausgenommen wird. Sonst
  spräche ein Agent, der sich Nummer 3 gemerkt hat, plötzlich über einen
  anderen Track.
- **`queue_next` legt ohne Deckangabe nur auf ein Deck, das nicht läuft.**
  Laufen alle, wird gefragt statt geraten; mit `queue_next deck1` geht es
  trotzdem, wenn es wirklich so gemeint ist. Und scheitert das Laden, kommt der
  Eintrag zurück nach vorn, statt still zu verschwinden.

Auch die Liste steht in der Regie-Spalte, mit `vor`, `weg` und `auflegen`; in
der Plattenkiste merkt ein `+` einen Track vor. Sonst wäre sie von der
Oberfläche aus nur lesbar und füllen könnte sie allein ein Agent.

## Was am Deck eingestellt wird, bleibt

Hot Cues liegen zur Laufzeit in den Atomics des Decks — dort sind sie richtig
aufgehoben, denn der Audio-Thread liest sie pro Block. Nur endete es dort auch:
Acht Cues gesetzt, Track neu geladen, weg. Und die Sammlung hatte die Tabelle
die ganze Zeit; der Traktor-Import füllte sie sogar, aber nichts las sie je.

```text
set deck1.cue1 4.5     → ok deck1.cue1 4.500000     (steht sofort in der Sammlung)
set deck1.cue1 -       → ok deck1.cue1 -            (Löschen wird genauso gespeichert)
```

**Sofort, nicht beim Beenden.** Ein Cue, der nur im Speicher steht, ist nach
einem Absturz weg, und abgestürzt wird beim Auflegen. Es sind acht Zeilen in
einer SQLite-Datei — das darf den Aufrufer kurz aufhalten, anders als das
Dekodieren beim Laden.

**Geschrieben werden nur die acht Tasten.** In derselben Tabelle liegen der
Grid-Anker aus dem Traktor-Import und Fade-Marker; ein Deck kennt die nicht und
darf sie deshalb nicht mit ersetzen (`Library::replace_hot_cues` statt
`replace_cues`).

### Das Beatgrid von Hand

Der Detektor liegt bei sperrigem Material daneben — bei einem Testtrack kam
180 BPM heraus, mit einer Konfidenz von 0,05. Ohne Korrektur wäre so ein Track
unbrauchbar:

```text
do deck1.grid_scale 0.5   → grid deck1 128.02 → 64.01 BPM
set deck1.position 2.5
do deck1.grid_here        → grid deck1 Anker auf 2.500 s
set deck1.bpm_grid 90     (geht auch direkt)
set deck1.grid_anchor 4.2
```

**`grid_scale` ist für den Oktavfehler da**, den mit Abstand häufigsten Fall;
der Anker bleibt dabei stehen, denn bei einer Halbierung lag jede zweite Eins
ohnehin richtig. **`grid_here` legt den Anker auf die Abspielposition** — der
klassische Handgriff, wenn man hört, wo die Eins liegt.

Genauer: auf die *Ziel*-Position. Ein `set position` setzt nur einen Wunsch,
den der Audio-Thread im nächsten Block ausführt; wer springt und sofort „hier"
sagt, meint die Stelle, auf die er gesprungen ist, nicht die, an der die
Anzeige noch steht.

Jede Korrektur landet sofort in der Sammlung, genau wie ein Cue. Ohne Grid
lässt sich kein Anker setzen und nichts skalieren — das wird gemeldet statt
geraten. Nur `bpm_grid` nimmt auch aus dem Nichts einen Wert an, sonst käme man
aus einem fehlenden Grid nie wieder heraus.

Beim Laden geht es rückwärts: Die gespeicherten Cues kommen aufs Deck, und ein
**Beatgrid aus der Sammlung schlägt die frische Analyse**. Was dort steht, kann
aus Traktor stammen oder von Hand korrigiert sein, und beides weiß mehr als ein
Detektor. Scheitert das Speichern — keine Sammlung geöffnet, Track nicht darin
—, steht der Cue trotzdem am Deck und der Grund in `deckN.load_status`.

## Wo im Track das Deck steht

Tempo, Tonart und Restbeats sagen, *wann* etwas geht. Sie sagen nicht, *wo* im
Stück man ist — und das ist die Frage, an der ein Übergang hängt. Deshalb kennt
jedes Deck seine Gliederung:

```text
get deck1.section              → value deck1.section drop
get deck1.section_beats_left   → value deck1.section_beats_left 28.97
get deck1.beats_to_outro       → value deck1.beats_to_outro 92.79
get deck1.intro_beats          → value deck1.intro_beats 32.00
get deck1.entry                → value deck1.entry 0.466417
do deck1.jump_entry            → jump_entry deck1 auf 0.466s
```

`section` ist eines von `intro`, `aufbau`, `drop`, `break`, `outro` oder `teil`.
Erkannt wird das offline beim Analysieren, auf dem Beatgrid — wie, steht in
`crates/analysis/src/struktur.rs`.

**`beats_to_outro` ist die Zahl, für die es das gibt.** Sie zählt herunter und
wird negativ, sobald das Outro läuft, und damit wird die eigentliche Regel
sagbar:

```text
when deck1.beats_to_outro < 0   in phrase ramp master.crossfader 1 32
```

Vorher hieß dieselbe Absicht „wenn noch 40 Beats übrig sind" — eine Zahl, die
man je Track neu schätzt und meistens falsch.

**`entry` ist der Einstiegspunkt: der erste Downbeat, nicht Sekunde 0.** Was
davor liegt, ist Vorlauf. Genau diesen Fehler hat die Mitschrift beim ersten
gemessenen Übergang aufgedeckt — Deck 2 setzte fünfzehn Beats neben seiner
eigenen Eins ein, weil es bei Frame 0 anfing. `do deckN.jump_entry` räumt das
in einem Griff weg.

**Ein frisch geladenes Deck steht dort inzwischen von selbst.** Wer von Hand
auflegt, cued auf den ersten Schlag; `do deckN.load` und `do master.queue_next`
tun jetzt dasselbe. Vorher stand das Deck auf Sekunde 0, und der nächste Griff
startete es dort — mitten in der Stille vor dem ersten Schlag.

Die Reihenfolge ist dabei nicht beliebig: Der Sprung muss **nach** dem
Stimmentausch kommen. Vorher gesetzt verbraucht ihn die alte Stimme auf ihrem
eigenen Track, und die neue fängt wieder bei null an. Gemessen am laufenden
Programm: `beat -1.02` gegen `beat -0.03`.

Steht der Abspielkopf im Vorlauf, ist `section` **leer**. Das ist keine Lücke,
sondern die Auskunft: Dort läuft noch kein Abschnitt. Dasselbe gilt für einen
Track ohne Outro — nicht jede Produktion blendet aus, und `beats_to_outro`
antwortet dann leer statt mit einer erfundenen Null.

**`intro_beats` ist für die Auswahl da.** „Ein Track mit langem Intro" ist damit
eine Anforderung, die ein Agent stellen kann, statt sie zu erraten.

## Wenn einer dem anderen dazwischenkommt

Eine Rampe gibt auf, sobald jemand anders denselben Regler anfasst. Nur erfuhr
das lange niemand: Der Taktgeber meldete es, und der Thread, der ihn aufruft,
warf die Meldung weg. Für einen einzelnen Bediener fiel das nie auf — er *war*
derjenige, der den Regler angefasst hat.

Für ein Team ist es der schlimmste denkbare Zustand: Zwei greifen nach demselben
Fader, einer verliert und plant weiter auf einer Annahme, die seit zwanzig
Sekunden falsch ist.

```text
sub master.events
ok sub 1 neu, 1 gesamt
…
event plan 3 abgeloest channel1.fader — jemand anders hat den Regler
event plan 4 fertig master.crossfader 1.0000
event plan 2 gestrichen
```

Gemeldet wird, was der Plan von sich aus tut: `fertig`, `abgeloest`,
`abgebrochen`, `ausgefuehrt` — und `gestrichen`, wenn jemand den Auftrag eines
anderen zurücknimmt. Die Zeilen kommen unaufgefordert, mit `event` davor; ein
Bediener, der sie nicht abonniert hat, bekommt nichts.

**Warum kein einzelner Wert.** Ein Control „letztes Ereignis" wäre einfacher
gewesen und falsch: Der Taktgeber läuft alle 5 ms, verglichen wird alle 50.
Zwischen zwei Blicken passen zehn Ereignisse, und neun davon wären weg —
ausgerechnet dann, wenn viel gleichzeitig geschieht. Stattdessen ein Ring mit
laufender Nummer; wer seine kennt, bekommt alles seither.

**Und wer zu langsam liest, erfährt das:**

```text
warnung 3 Ereignisse verloren — zu langsam gelesen
```

Dieselbe Regel wie bei `record_dropped` und den unlesbaren Zeilen der
Mitschrift. Eine Lücke, die aussieht wie keine, ist das schlechteste von allem.

**Wer nicht abonnieren kann, fragt.** Über MCP wird je Aufruf neu verbunden, ein
Abo hält dort nicht. Deshalb gibt es beides als Wert:

```text
get master.events        → value master.events plan 3 abgeloest channel1.fader
get master.event_count   → value master.event_count 17
```

Springt der Zähler zwischen zwei Blicken um mehr als eins, sind Zeilen
dazwischen liegengeblieben — und auch das ist besser sichtbar als still.

## Die Form einer Bewegung

Eine Rampe lief bisher immer gerade. Dieselbe Strecke lässt sich aber
verschieden verteilen, und das hört man:

```text
ramp master.crossfader 1 32 weich    # langsam los, durchziehen, weich ankommen
ramp channel1.eq_low 0 8             # ohne Angabe: linear
ramp channel2.fader 0.9 16 spaet deck1
```

| Form | Verlauf | Wofür |
| --- | --- | --- |
| `linear` | gleichmäßig | Bass-Swaps und alles, was nicht als Geste gemeint ist |
| `weich` | S-Kurve | lange Blenden — der Anfang unauffällig, die Mitte entschieden |
| `spaet` | lange fast nichts, dann schnell | hält den ausgehenden Track so lange wie möglich präsent |
| `frueh` | sofort viel, dann auslaufen | macht den Wechsel zum Ereignis |

**Deck und Form stehen hinten, in beliebiger Reihenfolge.** Erkannt werden sie
daran, *was* sie sind — die Namen sind disjunkt. Eine feste Reihenfolge wäre nur
eine Falle für den, der die Form angeben will, aber kein Deck, und das ist der
häufigere Fall.

Jede Form fängt bei 0 an und kommt bei 1 an, und keine läuft zwischendurch
zurück; ein Fader, der umkehrt, wäre kaputt und nicht ausdrucksstark. Bei
`weich` ist die Steigung an beiden Enden null — deshalb kein Ruck beim
Losfahren und keiner beim Ankommen.

Die Form steht auch im Plan:

```text
plan 3 ramp master.crossfader -1.0000 → 1.0000 über 32 Beats weich, 12.4 gelaufen (deck1)
```

Ohne das sähen zwei verschieden gemeinte Bewegungen für einen zweiten Bediener
gleich aus.

**Daneben gibt es `master.crossfader_curve`** — die Kennlinie des Crossfaders
selbst, von weich bis Schnitt. Sie stand im ersten automatisch gefahrenen Set
ungenutzt auf weich, und der Kritiker hat das Pegelloch in der Mitte prompt
gefunden. Form und Kurve sind zwei verschiedene Dinge: Die Form verteilt die
Bewegung über die Zeit, die Kurve übersetzt die Reglerstellung in Lautstärke.

## Ein Repertoire statt eines Handgriffs

Fünf benannte Übergänge:

```text
do master.uebergang bassswap 16
uebergang bassswap über 16 Beats, 6 Zeilen
  set channel2.eq_low 0 → ok channel2.eq_low 0
  in phrase set deck2.play 1 → ok plan 1 in 13.5 Beats: set deck2.play 1
  in phrase ramp master.crossfader 0 16 weich → ok plan 2 …
  in phrase+16 ramp channel1.eq_low 0 8 → ok plan 3 …
  in phrase+16 ramp channel2.eq_low 1 8 → ok plan 4 …
  in phrase+24 ramp master.crossfader 1 16 weich → ok plan 5 …
```

| Griff | Was er tut | Üblich |
| --- | --- | --- |
| `blende` | lange Blende über den Crossfader, weich verteilt | 32 Beats |
| `bassswap` | beide laufen in der Mitte, dann tauschen die Bässe | 16 |
| `schnitt` | auf der Eins umschalten, ohne Übergang | 0 |
| `filter` | dem Ausgehenden den Boden wegziehen, während der Neue kommt | 16 |
| `schleife` | den Ausgehenden in eine Schleife legen und darüber wechseln | 16 |

**Die Schleife ist der einzige Griff, der dem Ausgehenden Zeit gibt.** Ein
Track, der in vier Beats zu Ende wäre, hält so noch eine ganze Phrase durch —
genau dafür setzt ein Mensch am Ende eines Stücks eine Schleife: nicht, um
etwas zu wiederholen, sondern um nicht gehetzt wechseln zu müssen.

```text
in phrase set deck1.loop_beats 16
in phrase set deck2.play 1
in phrase ramp master.crossfader 1 16 weich
in phrase+16 set deck1.loop_active 0
```

Die Schleife ist genau so lang wie die Blende darüber: Der Ausgehende läuft sie
einmal durch, und wenn sie herum ist, ist der Wechsel fertig. Eine Schleife, die
länger steht als der Übergang, wiederholt hörbar — und das ist der Fehler, den
dieser Griff gerade vermeiden soll. Deshalb wird sie am Ende auch wieder
**gelöst**: Ein Deck, das im Hintergrund weiterschleift, ist beim nächsten Griff
eine Überraschung.

**Jeder Griff endet damit, dass das ausgehende Deck steht.** Das klang zuerst
nach einer Kleinigkeit und ist die Stelle, an der aus einem Übergang ein *Set*
wird: Bis dahin lief der ausgehende Track nach der Blende weiter — unhörbar
hinter dem geschlossenen Crossfader, aber laufend —, und der nächste
`uebergang` wurde deshalb abgewiesen: „es laufen mehrere Decks". Beim ersten
Griff fällt das nicht auf, beim zweiten sofort.

**Getaktet wird nach dem ausgehenden Deck.** Ohne Angabe nimmt `in` das erste
Deck mit Beatgrid — beim zweiten Griff also ausgerechnet das gerade gestoppte.
Ein stehendes Deck hält den Plan an, und der ganze Übergang stand still:
angenommen, eingetragen, nie gefahren. Das ausgehende Deck läuft dagegen per
Definition, solange der Griff dauert.

**Und keine Bewegung endet im selben Takt, in dem ihr Deck stehenbleibt.** Sonst
bekommt sie ihren letzten Schritt nicht mehr: Der Regler steht am Ziel, der
Auftrag aber für immer im Plan — sichtbar für jeden zweiten Bediener und nicht
mehr wegzubekommen, weil das Deck nicht mehr läuft. Deshalb ein Beat Nachlauf,
den niemand hört.

Drei Griffe hintereinander am laufenden Programm, ohne Handgriff dazwischen
außer Laden und `jump_entry`:

```text
get master.transitions → value master.transitions blende, bassswap, schleife
plan                   → ok 0 vorgemerkt
```

**Es geschieht nichts, was man nicht auch selbst tippen könnte.** Jeder Griff
ist eine Handvoll gewöhnlicher Zeilen, die durch denselben Weg laufen wie alles
andere — sie stehen im Plan, in der Mitschrift und bei den Ereignissen, ein
zweiter Bediener sieht sie kommen, und `cancel` nimmt sie zurück. Die Antwort
nennt jede davon, damit man sie lesen, ändern und beim nächsten Mal von Hand
anders setzen kann.

**Die Anlage wählt nicht aus.** Welcher Griff passt, hängt daran, ob der
ausgehende Track ein Outro hat, der eingehende ein langes Intro, wie groß der
Energieunterschied ist — und vor allem daran, was vorher schon dreimal gefahren
wurde. Seit der Gliederung stehen die Zahlen dafür im Steuerraum (`section`,
`intro_beats`, `beats_to_outro`); die Entscheidung gehört dem, der sie begründen
kann. Nähme die Anlage sie ab, verlöre das Set genau den Teil, um den es hier
geht.

Vorausgesetzt wird, dass **genau ein** Deck läuft und auf dem anderen etwas
liegt. Läuft nichts, gibt es keinen Übergang; laufen beide, ist er im Gange —
und wer dann gemeint ist, kann niemand wissen. In beiden Fällen sagt die Anlage
das, statt ein Deck zu wählen.

## Ein Set, das sich selbst weiterträgt

Die Teile ergeben zusammen mehr als ihre Summe. Ein Set ist Liste, Laden,
Griff — und keins davon braucht mehr eine Hand dazwischen:

```text
do master.queue_add /musik/mitte.wav
do master.queue_note 1 mehr Druck nach dem Aufbau
when deck1.beats_left < 200 do master.queue_next
when deck1.beats_to_outro < 0 do master.uebergang bassswap 16
```

Die erste Bedingung legt den nächsten Track auf, sobald der laufende sich dem
Ende nähert — auf dem freien Deck, auf dem ersten Downbeat. Die zweite fährt den
Übergang, sobald das Outro anfängt. Danach steht das ausgehende Deck, der Plan
ist leer, und dieselben zwei Zeilen lassen sich für den nächsten Track wieder
setzen.

**Gewählt hat trotzdem ein Mensch oder ein Agent**, und zwar zweimal: welcher
Track in die Liste kommt — mit Notiz, warum — und welcher Griff gefahren wird.
Die Anlage führt aus und begründet, was sie gerade tut; sie entscheidet nicht,
was als Nächstes gut wäre. `do master.search_next` sortiert dafür die Sammlung,
`master.why` sagt den Satz dazu, `master.repeats` warnt vor Eintönigkeit.

**Was dabei schiefgehen kann, sagt die Anlage.** Beim ersten selbstausgelösten
Übergang lief der ausgehende Track mitten im Bass-Swap aus; seitdem bricht der
Plan ab und meldet `deck1 ist durchgelaufen`, statt mit dem Crossfader in der
Mitte stehenzubleiben. Gegen den Fall selbst gibt es den Griff `schleife`.

## Der Bogen: was das Set vorhat

Ein einzelner guter Übergang ist Handwerk. Ein gutes Set ist Architektur —
Aufbau, Plateau, Bruch, Wiederaufbau, über eine Stunde und nicht über vier
Minuten. Bisher konnte die Anlage jeden Übergang begründen und keine
Reihenfolge.

```text
set master.arc 0 0.3, 20 0.7, 45 0.95, 60 0.5, 80 1.0
do master.arc_start
get master.arc_gap        → value master.arc_gap 0.400000
get master.arc_trend      → value master.arc_trend steigt
```

Zeiten in **Minuten**, Energie zwischen 0 und 1. Zwischen den Punkten wird
geradlinig verbunden — nicht weil das musikalisch stimmt, sondern weil alles
andere eine Genauigkeit vortäuschte, die eine von Hand gesetzte Kurve nicht hat.

| Control | Was |
| --- | --- |
| `arc` | die Kurve selbst, schreibbar |
| `arc_minutes` | wie lange das Set läuft |
| `arc_target` | was der Bogen hier vorsieht |
| `arc_actual` | was gerade läuft |
| `arc_gap` | **Soll minus Ist** — die Zahl, nach der gewählt wird |
| `arc_trend` | `steigt`, `haelt` oder `faellt` |

Damit wird die eigentliche Frage sagbar:

```text
when master.arc_gap > 0.3 do master.queue_next
```

**Die Ist-Energie kommt aus der Art des Abschnitts, nicht aus dem Pegel.** Die
Gliederung misst je Abschnitt einen Pegel, aber der ist auf den lautesten
Abschnitt *desselben* Tracks bezogen: Der Drop eines leisen Stücks steht dort
genauso bei 0,99 wie der eines lauten. Über Tracks hinweg ist das nicht
vergleichbar, und ein Bogen, der solche Zahlen addiert, rechnet mit Äpfeln.
Stattdessen eine grobe Leiter — Intro 0,2, Break 0,35, Aufbau 0,55, Drop 0,9 —,
und grob ist hier ehrlicher als eine Nachkommastelle, die niemand einlösen kann.

**Ohne `arc_start` gibt es keinen Ort auf dem Bogen**, und dann wird auch keiner
behauptet: `arc_minutes`, `arc_target` und `arc_gap` antworten leer. Eine Kurve
ohne Uhr ist ein Bild, kein Maßstab.

**Für ein Team ist der Bogen das, worüber man sich einig sein muss.** Ohne ihn
verhandeln zwei Agenten über den nächsten Track ohne gemeinsamen Maßstab: Der
eine will Druck, der andere Luft, und beide haben recht, weil es keinen Satz
gibt, gegen den sich das prüfen ließe. Mit ihm heißt die Frage nicht mehr
„welcher Track ist gut", sondern „was fehlt hier gerade".

## Einzelspuren: die Stimme wegnehmen

Zwei Stimmen gleichzeitig sind der hörbarste Mixfehler überhaupt, und ohne
Trennung lässt er sich nur vermeiden, indem man gar nicht überlagert.

```text
get deck1.stems        → value deck1.stems 3
get deck1.stem3_name   → value deck1.stem3_name vocals
set deck1.stem3_level 0
```

| Control | Was |
| --- | --- |
| `stems` | wie viele Einzelspuren der geladene Track hat |
| `stemN_name` | wie die Spur heißt — aus dem Dateinamen |
| `stemN_level` | ihr Pegel, 0 bis 1; schreibbar |

**Wo die Spuren liegen:** neben der Datei in einem Ordner gleichen Namens mit
der Endung `.stems` — zu `nachtschicht.wav` also `nachtschicht.stems/` mit
`vocals.wav`, `drums.wav`, `bass.wav`, `other.wav`. Kein neues Dateiformat,
keine neue Abhängigkeit, und genau die Form, in der die gängigen Trennwerkzeuge
ihr Ergebnis ablegen. Ein eigenes Stem-Format zu lesen wäre die Alternative
gewesen: eine MP4-Datei mit fünf AAC-Spuren, die einem Hersteller gehört. Ein
Ordner mit vier WAV-Dateien gehört niemandem.

**Getrennt wird hier nicht.** Eine Stimme aus einer Mischung zu lösen ist Arbeit
für ein neuronales Netz, ein eigenes Werkzeug und eine eigene Lizenzfrage.
Gelesen wird, was jemand anders getrennt hat. Ohne Ordner verhält sich alles wie
vorher — kein Sonderweg, den jemand einschalten müsste.

**Die Zeitstreckung entscheidet einmal für alle Spuren.** Sie sucht sich in
jedem Hop die Stelle, an der die Wellenform am besten anschließt; liefe diese
Suche je Spur getrennt, fände jede eine andere, und was vorher zusammen klang,
klänge verwaschen. Gesucht wird deshalb auf der Summe, angewandt auf jede Spur.
Ein Wächter misst das: Mit vollen Pegeln muss dasselbe herauskommen wie beim
Strecken der Summe, Ton für Ton.

Gemessen am laufenden Programm, zwei Mitschnitte desselben Stücks:

| | Energie bei 1500 Hz | Gesamtpegel |
| --- | --- | --- |
| mit Stimme | 9,8 · 10⁻⁵ | 0,061 |
| ohne Stimme | 4,9 · 10⁻⁹ | 0,039 |

Die Stimme ist um den Faktor 20 000 weg, Drums und Bass stehen.

**Was noch fehlt: Streaming von Platte.** Vier Spuren kosten das Fünffache eines
Tracks, weil die Summe daneben stehen bleibt — rund 500 MB je Deck bei fünf
Minuten. Für zwei Decks geht das; für die vier, die der Plan vorsieht, braucht
es Streaming. Das steht bewusst noch aus: Der Abspielpfad ist der einzige Teil
mit Echtzeitauflagen, und Platten-I/O gehört dort zuletzt hinein.

## Der Raum: was draußen geschieht, verschiebt das Ziel

Die vier Signalplätze gab es lange, aber sie waren Deko — Werte gingen hinein,
und nichts hat darauf reagiert. Ein Bogen, der von der ersten Minute an
feststeht, ist ein Abspielplan.

```text
set master.signal1_name Andrang
set master.room signal1 0.5
set master.signal1 0.6            # und später wieder, und wieder

get master.arc_curve  → value master.arc_curve 0.700000
get master.room_bend  → value master.room_bend -0.200000
get master.arc_target → value master.arc_target 0.500000
```

| Control | Was |
| --- | --- |
| `room` | welches Signal den Bogen beugt und wie stark, schreibbar; `-` schaltet ab |
| `room_bend` | wie weit der Raum das Ziel gerade verschiebt |
| `arc_curve` | was die geschriebene Kurve hier vorsieht — ungebeugt |
| `arc_target` | was gerade angestrebt wird: Kurve **plus** Raum |
| `why` | in einem Satz, woraus das gerechnet wurde |

**Der Raum verschiebt das Ziel, nicht die Kurve.** Das ist der ganze
Unterschied zwischen „der Raum redet mit" und „der Raum schreibt um": Am Ende
des Abends steht in `arc` immer noch der Plan, gegen den sich vergleichen
lässt, was tatsächlich geschehen ist. Gebeugt wird `arc_target` und damit
`arc_gap` — die Zahl, nach der gewählt wird.

**Gebeugt wird nach dem Trend, nicht nach dem Wert.** Die Höhe eines Signals
ist nichts Vergleichbares: Was „0,6 Andrang" bedeutet, weiß nur, wer den Sender
geschrieben hat, und eine Anlage, die diese Zahl direkt gegen die Energie des
Bogens rechnet, addiert Äpfel — derselbe Fehler, der beim Ist-Wert schon einmal
auffiel. Eine *Änderung* ist dagegen eine Aussage über denselben Sender: „seit
drei Minuten fallend" heißt dasselbe, egal wie die Skala gemeint war.

**Höchstens 0,25.** Ohne Deckel könnte ein hängender oder falsch skalierter
Sender das Set übernehmen, und das wäre schlimmer als ein Set, das den Raum
ignoriert. Wer mehr Wirkung will, sagt das über das Gewicht; über die Grenze
kommt er trotzdem nicht.

### Und was daraus folgt

```text
do master.search_next
weil Andrang fällt (-0.40/min), Ziel 0.70 → 0.50; es läuft 0.90, weniger gesucht (-0.40)
track 126.00 8A /musik/blaue-stunde.wav Blaue Stunde — Energie 0.48 (-0.02 zum Ziel 0.50), harmonisch
track 128.00 9A /musik/kellerlicht.wav Kellerlicht — Energie 0.61 (+0.11 zum Ziel 0.50), harmonisch
track 127.00 3A /musik/nachtschicht.wav Nachtschicht — Energie 0.88 (+0.38 zum Ziel 0.50), tonartfremd
```

Bis hierher konnte der Raum ein Ziel verschieben, aber gewählt wurde weiter nach
Tempo und Tonart allein — zwei Größen, die nichts darüber sagen, ob es gerade
lauter oder ruhiger werden soll. `search_next` sortiert nach dem Abstand zum
gebeugten Ziel und sagt je Zeile, warum sie dort steht.

Die Energie eines Tracks kommt aus seiner **Gliederung**, nach Länge gewichtet:
Ein Stück mit kurzem Drop und langem Outro ist ruhiger als eines, bei dem es
umgekehrt ist. Sie steht nicht in der Datenbank, sondern in der Analyse-Datei
neben dem Track — sie hängt am Inhalt, nicht am Eintrag. **Ein nicht
analysierter Track steht hinten und sagt das**, statt sich mit einer erfundenen
mittleren Energie in die Mitte zu mogeln.

**Diese Zahl misst keine Lautstärke und keine Härte.** Die Abschnittsarten
werden je Track gegen dessen *eigene* Quantile benannt — der Drop eines leisen
Stücks heißt genauso „Drop" wie der eines lauten. Was herauskommt, ist: wie viel
seiner Länge ein Track auf seinem eigenen Höhepunkt verbringt. Für den Aufbau
eines Sets ist das die brauchbarere Größe — ein Peak-Track sitzt die meiste Zeit
oben, ein Warmup-Track baut die meiste Zeit auf. Auf die Frage „welches Stück
ist härter?" antwortet sie nicht, und das ist besser, als sie falsch zu
beantworten.

**Ausgewählt wird trotzdem nicht.** Die Anlage sortiert; welcher Track es wird,
entscheidet, wer es begründen kann. Ein `search_next`, das selbst lädt, wäre
etwas anderes — und genau das, was hier nicht gebaut wird.

## Zurückhaltung: ob immer dasselbe gefahren wird

Das Repertoire macht Abwechslung **möglich**. Es erzwingt sie nicht — und ein
System, das viermal hintereinander dieselbe Blende wählt, klingt weiterhin nach
Automat, auch wenn jede einzelne sauber ist. Der Vorwurf an den ersten
selbstgefahrenen Übergang („ein bisschen herzlos") war nie mit einer Zahl
beantwortet. Hier ist sie.

```text
get master.repeats       → value master.repeats 3
get master.transitions   → value master.transitions blende, blende, blende
```

| Control | Was |
| --- | --- |
| `repeats` | wie viele der letzten Übergänge hintereinander gleich waren |
| `transitions` | die letzten acht, ältester zuerst |

`1` heißt: der letzte war anders als der davor, alles in Ordnung. Ab `3`
wiederholt sich jemand hörbar, und das lässt sich zur Bedingung machen:

```text
when master.repeats > 2 do master.uebergang filter
```

**Gezählt wird der Seitenwechsel des Crossfaders, nicht sein Wert.** Das ist die
einzige Stelle, die sich ohne Raterei bestimmen lässt, und sie zählt genau
einmal je Übergang: Ein Bass-Swap fährt zweimal am Crossfader (erst in die
Mitte, dann hinüber), eine Blende in achtzig kleinen Schritten, ein Schnitt in
einem Sprung — angekommen sind alle drei einmal. Zählte man stattdessen den
Wert, stünde nach einer einzigen Blende eine Wiederholung von 80 da, und die
Zahl, die vor Eintönigkeit warnen soll, wäre selbst der Grund, sie zu
ignorieren.

Wie ein Übergang heißt, entscheidet sich beim Ankommen: Wurde er über
`master.uebergang` angefordert, steht sein Name da (`bassswap`), sonst die
Bewegung, die den Fader hinübergebracht hat (`weich/32`) oder `schnitt`. Damit
zählt auch, was **von Hand** zusammengesetzt wurde — der erste automatisch
gefahrene Übergang bestand aus sieben `when`-Zeilen und keinem einzigen
`uebergang`. Ein Griff, der vorgemerkt und dann abgelöst wird, zählt nicht: Er
hat nicht stattgefunden.

**Und es müssen zwei Decks laufen.** Wer vor dem Set den Fader auf die Seite des
einzigen laufenden Decks stellt, richtet ein — da ist nichts, wovon oder wohin
überzublenden wäre. Das stand zunächst falsch da und fiel erst am laufenden
Programm auf: Nach vier Zeilen Einrichten meldete die Anlage bereits einen
Übergang.

**Zwei Grenzen sind echt:** Wer nur mit den Kanalfadern mischt und den
Crossfader stehen lässt, taucht hier nicht auf. Und wenn der ausgehende Track
ausläuft, bevor die Blende drüben ankommt, fehlt sie — dann lief zuletzt nur
noch ein Deck. Beides kommt vor; dann sagt diese Zahl nichts, statt etwas
Falsches zu sagen.

## Harmonisch mischen

Tempo ist die halbe Trackauswahl. Zwei Stücke im gleichen Takt können trotzdem
gegeneinander klingen, wenn die Tonarten nicht zusammenpassen — deshalb gibt es
sie am Deck und in der Sammlung:

```text
get deck1.key            → value deck1.key Am
get deck1.key_camelot    → value deck1.key_camelot 8A
do master.search_harmonic 8A
                         → track 127.98 8A /musik/01.wav  Nachtschicht
                           track 125.99 8B /musik/02.wav  Alpenglühen
                           track 123.99 9A /musik/03.wav  Blaue Stunde
```

**Zwei Felder statt einem.** `key` ist die Schreibweise für Menschen (`Am`,
`F#`), `key_camelot` die zum Rechnen (`8A`, `2B`). Beides in eine Zeichenkette
zu packen hieße, dass jeder Leser sie wieder auseinandernehmen muss.
`search_harmonic` nimmt beide entgegen, und was keine Tonart ist, wird
**gemeldet** statt als leeres Ergebnis zurückzukommen — ein Tippfehler darf
nicht aussehen wie eine leere Sammlung.

Gesucht wird nach der Regel des Camelot-Rads: dieselbe Zahl (Paralleltonart),
eine weiter, eine zurück. Deshalb steht in jeder Trefferzeile jetzt eine
Camelot-Spalte hinter dem Tempo.

**Leer heißt unbekannt, nicht „passt zu allem".** Auf Bass und Drums allein
ermittelt die Analyse bewusst keine Tonart — warum, steht in
`crates/analysis/src/tonart.rs`. Diese Tracks tauchen in einer harmonischen
Suche nicht auf; das ist richtig so, denn ein geratener Wert würde beim Mischen
mehr kosten als ein fehlender.

## Auch die Oberfläche geht hier durch

Nicht nur die Regler: Suchen und Laden nehmen inzwischen denselben Weg. Ein
Klick auf „A" in der Plattenkiste löst `deck1.load` aus, das Suchfeld ruft
`master.search`, und „Harmonisch zu A" ruft `master.search_harmonic` mit der
Tonart von Deck 1. Auch die Cue-Knöpfe und die Grid-Korrektur gehen diesen Weg
— sie schrieben vorher direkt ins Deck, und genau dort wären sie an der
Sammlung vorbeigelaufen.

Das ist kein Selbstzweck. Vorher hatte die Oberfläche einen eigenen Ladepfad —
zwei Wege zum selben Ziel heißt zwei Stellen, an denen es schiefgehen kann, und
die seltener benutzte fällt seltener auf. Jetzt gilt: Was ein Agent kann, kann
die Oberfläche auch, und umgekehrt. Bricht der eine Weg, bricht der andere
sofort mit — und wird bemerkt.

## Was noch fehlt

- **Reverb.** Vier Effekte gibt es; ein guter Hall fehlt und ist ein eigenes
  Stück Arbeit.
- **Beatgrid korrigieren.** `set deck1.bpm_grid` ist nur lesbar; ein falsch
  erkanntes Grid lässt sich von außen nicht geraderücken.
- **Windows: Named Pipe.** Die Steuerung läuft dort inzwischen über die
  Rückschleife (`--tcp`, mit Schlüssel), also gibt es sie überhaupt. Eine Named
  Pipe wäre trotzdem der bessere Weg: Sie erbt die Rechte des Systems, so wie
  der Unix-Socket, und käme ohne Schlüssel aus.
- **MIDI.** Der normierte Weg (`setn`) ist da, der Übersetzer von MIDI-CC auf
  Control-Namen noch nicht. Das ist Phase 10 — und mit dem Steuerraum im Rücken
  fast nur noch eine Tabelle.
- **MCP.** Die Werkzeugbeschreibungen lassen sich aus dem Katalog erzeugen; der
  Erzeuger fehlt noch.
