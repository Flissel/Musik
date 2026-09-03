# Freigabe: was ein Agent von sich aus darf

Spezifikation, kein Bericht. Sie beschreibt, was gebaut werden soll, und ist so
geschrieben, dass sich hinterher prüfen lässt, ob es gebaut wurde.

Stand: **gebaut**, September 2026. Was hier steht, steht auch im Code —
`mcp/musik/freigabe.py`, `mcp/musik/tests/test_freigabe.py`, `note` in
`crates/control/src/protokoll.rs`.

Am laufenden Programm nachgefahren: ohne Freigabe abgewiesen, mit einer für
`mischen` durchgelassen, `datei` daneben weiterhin zu; zwei Freigabe-Fenster
ergaben zwei Notizen in der Mitschrift, fünf Aufrufe unter einem Fenster genau
eine; und die Datei mitten im Betrieb gelöscht wirkte beim nächsten Aufruf,
ohne Neustart.

## Warum überhaupt

VibeMind lässt lesende MCP-Werkzeuge frei laufen und schickt schreibende durch
ein Gatter (`mcp/docker/approval.py`, Default-Deny, Erlaubnisliste in der
Umgebung). Die Musik-Brücke kennt das nicht: Alle sechzehn Werkzeuge fahren
sofort. Als eigenständiges Werkzeug hinter einem Unix-Socket ist das richtig —
wer den Socket öffnen darf, darf auch auflegen. Als Space wäre es ein Bruch mit
dem Sicherheitsmodell der Umgebung.

Der naheliegende Weg — jeder Schreibzugriff einzeln — ist für ein Pult
unbrauchbar. Sechzehn Takte sind bei 128 BPM dreißig Sekunden, und ein Übergang
besteht aus sieben Zeilen, die auf Phrasengrenzen fallen müssen. Wer dazwischen
auf eine Bestätigung wartet, hat keinen Übergang, sondern einen Abbruch.

Die Auflösung ist keine Ausnahme vom Modell, sondern eine andere **Körnung**:
Freigegeben wird ein Bereich für die Dauer eines Sets, nicht ein Aufruf.

## Wo das Gatter sitzt — und wogegen es nicht hilft

Es sitzt in der **MCP-Brücke**, nicht im Steuerraum.

Der Steuerraum ist die Schnittstelle für alles, was bedient: Oberfläche,
`nc`, Agenten. Ein Gatter dort träfe die Hand am Regler genauso wie das Modell,
und das wäre falsch — wer den Socket öffnen darf, ist bereits autorisiert.

Daraus folgt eine Grenze, die dazugehört:

> **Das Gatter schützt davor, dass ein Agent seine Werkzeuge missbraucht. Es
> schützt nicht vor etwas, das bereits Socket-Zugang hat.** Ein Agent mit einem
> Shell-Werkzeug oder einem freien Dateizugriff umgeht es. In VibeMind sind
> Werkzeuge einzeln freigeschaltet; dass diese beiden nicht dabei sind, ist
> Aufgabe dessen, der den Space einrichtet, und steht deshalb in `AGENTS.md`
> als harte Regel.

Eine Spezifikation, die diese Grenze verschweigt, verspricht Sicherheit, die
sie nicht hat.

## Vier Klassen

Klassifiziert wird nach dem, was ein Aufruf **berührt**, nicht nach seinem
Namen. Es gibt über zweihundert Controls; eine Liste von Namen wäre nach dem
nächsten Feature falsch.

| Klasse | Was hineinfällt | Voreinstellung |
| --- | --- | --- |
| `lesen` | `musik_status`, `musik_list_controls`, `musik_get`, `musik_search`, `musik_next`, `musik_queue` | **frei** |
| `mischen` | Fader, Trim, EQ, Filter, Crossfader, Cue, Summenlautstärke, Stem-Pegel | Set-Freigabe |
| `spielen` | `play`, `cue`, `jump_*`, `sync`, `tempo`, `keylock`, Loops, Hot Cues | Set-Freigabe |
| `zeit` | `musik_ramp`, `musik_schedule`, `musik_when`, `musik_uebergang`, `musik_cancel` | Set-Freigabe |
| `datei` | `master.record`, `record_stop`, `deckN.load`, `musik_queue_add` | **einzeln** |
| `dramaturgie` | `master.arc`, `arc_start`, `master.room`, `musik_signal`, `queue_note` | **einzeln** |
| `erzeugen` | Generator-Aufrufe (siehe [GENERATOR.md](GENERATOR.md)) | **nie ohne Mensch** |

Sieben Klassen, vier Voreinstellungen. Die Trennung von `mischen` und `spielen`
sieht nach Haarspalterei aus und ist keine: Ein Agent, der nur mischen darf,
kann einen laufenden Übergang zu Ende fahren, aber keinen neuen Track starten.
Das ist eine sinnvolle Zwischenstufe für einen Assistenten am Pult.

**`datei` und `dramaturgie` sind bewusst nicht in der Set-Freigabe.** Ein
Ladevorgang nimmt einen Pfad entgegen und berührt das Dateisystem; der Bogen
legt fest, was das Set überhaupt vorhat. Beides ist kein Handgriff, der in
dreißig Sekunden geschehen muss.

## Die Set-Freigabe

### Sie kommt von außen, nicht über MCP

Ein Werkzeug `musik_freigabe_erteilen` wäre sinnlos: Ein Agent, der sich selbst
freigeben kann, ist nicht eingeschränkt. Die Freigabe kommt deshalb aus einer
**Datei, die der Agent nicht anfassen kann** — ihr Pfad steht in
`MUSIK_FREIGABE_DATEI`.

Eine Datei und keine Umgebungsvariable, aus einem einzigen Grund: **Widerrufen
muss ohne Neustart gehen.** Eine Variable steht fest, sobald der Prozess läuft.
Wer mitten im Set merkt, dass der Agent Unsinn macht, löscht die Datei, und der
nächste Aufruf ist abgewiesen.

### Format

```text
# Freigabe für den Freitagabend
klassen mischen spielen zeit
bis    2026-09-03T23:30:00Z
von    felix
```

- `klassen` — durch Leerzeichen getrennt, aus der Tabelle oben.
- `bis` — RFC 3339, **mit Zeitzone**. Ohne Zeitzone wird die Zeile abgewiesen,
  nicht als Ortszeit geraten.
- `von` — freier Text, landet in der Meldung und in der Mitschrift.
- Zeilen mit `#` und Leerzeilen werden übergangen.

Gelesen wird **bei jedem Aufruf**. Das kostet einen `stat` und einen kurzen
Read; gegen einen Werkzeugaufruf, der ohnehin über eine Prozessgrenze geht, ist
das nichts. Zwischenspeichern wäre genau die Optimierung, die das Widerrufen
wieder kaputt macht.

### Wann sie nicht gilt

Fail-closed, in jedem dieser Fälle:

- `MUSIK_FREIGABE_DATEI` ist nicht gesetzt
- die Datei fehlt oder ist nicht lesbar
- `bis` fehlt, ist unparsbar oder liegt in der Vergangenheit
- `bis` liegt mehr als **zwölf Stunden** in der Zukunft
- `klassen` fehlt oder enthält ein unbekanntes Wort

Die Zwölf-Stunden-Grenze ist kein Sicherheitsmerkmal, sondern eine Bremse gegen
den bequemsten Fehler: eine Freigabe „bis 2099" einmal schreiben und nie wieder
daran denken. Wer länger braucht, schreibt die Datei neu.

**Ein unbekanntes Wort in `klassen` verwirft die ganze Datei**, statt den Rest
gelten zu lassen. Ein Tippfehler in `mischn` soll auffallen und nicht dazu
führen, dass drei von vier Klassen still funktionieren.

## Was bei einer Ablehnung zurückkommt

Nach dem Vorbild von `mcp/docker/approval.py`: Die Antwort sagt **genau, was
gelaufen wäre**, und wie man es erlauben würde.

```text
freigabe verweigert für: set channel1.fader 0.8
  Klasse: mischen
  Grund:  keine gültige Freigabe (MUSIK_FREIGABE_DATEI nicht gesetzt)
  Erlaubt: klassen mit mischen in MUSIK_FREIGABE_DATEI eintragen
```

Drei Regeln dazu:

1. **Kein Ausführen und dann Melden.** Geprüft wird vor dem Verbindungsaufbau
   zum Pult.
2. **Die Ablehnung verrät nichts über den Zustand der Anlage.** Kein „Deck 1
   läuft ohnehin nicht" — wer nicht schreiben darf, erfährt auch nichts.
3. **Lesende Werkzeuge werden nie abgelehnt.** Sie sind frei; ein Gatter davor
   wäre Verwaltung ohne Gewinn.

## Was in die Mitschrift gehört

Die Mitschrift hält heute jede Zeile fest, die durch das Pult geht — ein
freigegebener Aufruf steht also von selbst darin. Was fehlt, ist die **Freigabe
selbst**: Wer sie erteilt hat und bis wann.

Dafür braucht das Protokoll einen Befehl, den es noch nicht gibt:

```text
note freigabe mischen spielen zeit bis 23:30 von felix
```

`note <text>` schreibt eine Zeile in die Mitschrift und tut sonst nichts. Er
gehört in die Klasse `lesen` — er ändert nichts an der Anlage. Die Brücke
schickt ihn **einmal je Freigabe-Fenster**, beim ersten Aufruf, der unter dieser
Freigabe läuft; nicht bei jedem, sonst steht die Mitschrift voll.

Ohne diesen Befehl ist hinterher nicht rekonstruierbar, unter welcher Vollmacht
ein Set gefahren wurde. Das ist genau die Frage, die man stellt, wenn etwas
schiefging.

## Was gebaut wird

| Datei | Inhalt |
| --- | --- |
| `mcp/musik/freigabe.py` | Klassen, Datei lesen, `pruefen(...)`, `pruefen_control(...)`, `verweigert(...)` |
| `mcp/musik/controls.txt` | alle 194 Controls, erzeugt aus `list`; ein Test hält dagegen, dass jedes eine Klasse hat |
| `mcp/musik/server.py` | je Werkzeug eine Klasse; Aufruf des Gatters vor `sprich(...)` |
| `crates/control/src/protokoll.rs` | `note <text>` |
| `crates/control/src/katalog.rs` | `note` im Katalog, damit `list` ihn kennt |
| `mcp/musik/tests/test_freigabe.py` | die Tests unten |

## Tests, die dafür stehen

Ohne diese Liste wäre die Spezifikation eine Absichtserklärung. Sie laufen in
der CI (`mcp/musik/tests/test_freigabe.py`, 18 Stück — die zwölf unten, die Ablehnung
selbst und fünf über die Zuordnung der Controls).

**Fail-closed:**

1. Ohne `MUSIK_FREIGABE_DATEI` wird jede schreibende Klasse abgewiesen.
2. Fehlende Datei → abgewiesen.
3. Abgelaufenes `bis` → abgewiesen, und die Meldung nennt den Ablauf.
4. `bis` ohne Zeitzone → abgewiesen, nicht als Ortszeit geraten.
5. `bis` mehr als zwölf Stunden voraus → abgewiesen.
6. Unbekanntes Wort in `klassen` → **die ganze Datei** ungültig, nicht nur das Wort.

**Was durchgeht:**

7. Gültige Datei mit `mischen` → `set channel1.fader` läuft, `deck1.load` nicht.
8. Lesende Werkzeuge laufen in allen sechs Fällen oben.
9. `erzeugen` läuft **nie**, auch wenn es in `klassen` steht — die Klasse ist
   über die Datei nicht erreichbar.

**Widerruf:**

10. Datei löschen, während der Prozess läuft → der nächste Aufruf ist
    abgewiesen, ohne Neustart.

**Mitschrift:**

11. Unter einer Freigabe steht genau **eine** `note`-Zeile, auch nach zehn
    Aufrufen.
12. Eine neue Freigabe-Datei (anderes `bis`) erzeugt eine neue `note`-Zeile.

Punkt 9 ist der wichtigste: `erzeugen` gehört nicht in die Freigabe-Datei,
sondern hinter eine Bestätigung je Aufruf. Ein Test, der das festhält,
verhindert, dass die Klasse später „aus Bequemlichkeit" dazukommt.

## Was diese Spezifikation nicht regelt

- **Wer die Datei schreiben darf.** Das sind Dateisystemrechte, und die gehören
  dem Betriebssystem, nicht dieser Brücke.
- **Mehrere Agenten mit verschiedenen Rechten.** Heute gilt eine Freigabe für
  die Brücke, nicht je Sitzung. Zwei Modelle mit verschiedenen Vollmachten wären
  eine eigene Aufgabe — und sie hängt an P2 Stufe 2, die noch aussteht.
- **Widerruf mitten in einer laufenden Rampe.** Eine Rampe, die schon im Plan
  steht, läuft zu Ende; die Freigabe wirkt auf neue Aufrufe. Wer sie sofort
  stoppen will, nimmt `cancel` — von Hand, über den Socket.
