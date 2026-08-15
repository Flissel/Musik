# musik_mcp

Ein MCP-Server, der die laufende DJ-Anwendung an einen Agenten weiterreicht.

Die eigentliche Arbeit macht `musik-app`; dieser Server ist ein dünner
Übersetzer zwischen MCP und dem Zeilenprotokoll auf dem Unix-Socket
(→ [docs/STEUERUNG.md](../docs/STEUERUNG.md)).

## Warum Python neben einem Rust-Projekt

Weil der Empfänger Python spricht: [VibeMind](../docs/VIBEMIND.md) ist die
Stelle, an die das hier andockt, und dort läuft FastMCP. Der Rust-Kern bleibt
unangetastet — die Brücke redet nur über den Socket und weiß von Audio nichts.

Wer keinen Agenten braucht, braucht diesen Ordner nicht. `musik-app` läuft ohne
ihn, und `nc -U` tut es für die Hand auch.

## Einrichten

```sh
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

In einem MCP-Client (Claude Desktop, VibeMind, …):

```json
{
  "mcpServers": {
    "musik": {
      "command": "/pfad/zu/.venv/bin/python",
      "args": ["/pfad/zu/musik/mcp/musik_mcp.py"],
      "env": { "MUSIK_SOCKET": "/run/user/1000/musik.sock" }
    }
  }
}
```

`MUSIK_SOCKET` ist optional; ohne die Variable wird
`$XDG_RUNTIME_DIR/musik.sock` genommen — der Standard von `musik-app`.

## Werkzeuge

| Werkzeug | Wofür |
| --- | --- |
| `musik_status` | Momentaufnahme: Decks, Kanalzüge, Summe, Mitschnitt, Plan |
| `musik_list_controls` | Was es gibt — mit Bereich, Einheit und Bedeutung |
| `musik_get` | Einen Wert lesen |
| `musik_search` | Sammlung durchsuchen, nach Text, Tempo oder Tonart |
| `musik_set` | Einen Wert setzen |
| `musik_do` | Eine Aktion auslösen: laden, syncen, Cue, Mitschnitt |
| `musik_ramp` | Einen Regler über Beats bewegen — eine Blende |
| `musik_schedule` | Eine Aktion oder einen Wert auf einen späteren Beat legen |
| `musik_cancel` | Vorgemerktes zurücknehmen |
| `musik_queue` | Was als Nächstes kommt — mit Notiz, warum |
| `musik_queue_add` | Einen Track vormerken |
| `musik_queue_next` | Den vordersten auflegen |

Zwölf statt zweihundert. Ein Werkzeug je Control wäre die naheliegende
Übersetzung und die schlechtere: Der Steuerraum hat über zweihundert Einträge,
und ein Agent, der sie alle als Werkzeuge sieht, findet keins davon.
`musik_set` und `musik_do` erreichen alles, `musik_status` und `musik_search`
sparen die Wege, die man sonst am häufigsten doppelt ginge.

## Warum es Zeit-Werkzeuge braucht

`musik_set` allein reicht für Reglerstellungen, nicht für Übergänge. Eine
Blende ist eine Bewegung über Takte; wer sie von außen nachbaut, ruft
`musik_set` in einer Schleife und schläft dazwischen — über eine
Werkzeugschnittstelle, deren Timing bei jedem Aufruf eine Modellantwort weit
entfernt ist. Das eiert hörbar, und der Agent kann in der Zeit nichts anderes
tun.

`musik_ramp` und `musik_schedule` sagen einmal, was passieren soll, und kommen
sofort zurück. Gerechnet wird in **Beats, nicht in Sekunden**: Dreht jemand am
Tempo, bleibt die Blende musikalisch richtig; steht das Deck, wartet sie.

```text
musik_ramp(control='channel1.eq_low', ziel=0, ueber_beats=8)
musik_ramp(control='master.crossfader', ziel=1, ueber_beats=32, in_beats=16)
musik_schedule(beats=64, aktion='deck2.sync')
```

## Der Plan ist das gemeinsame Blatt

Selten bedient nur einer. Deshalb steht im Feld `plan` von `musik_status`, was
vorgemerkt ist — von wem auch immer:

```json
"plan": [
  {"id": 1, "art": "ramp", "text": "ramp channel1.eq_low 1.0000 → 0.0000 über 8 Beats, 2.5 gelaufen (deck1)"},
  {"id": 2, "art": "in",   "text": "in 61.5 Beats: do deck2.sync (deck1)"}
]
```

Wer das liest, greift nicht mitten in eine fremde Blende. Und wenn doch: **Der
letzte Griff gewinnt.** Eine Rampe gibt auf, sobald jemand anders denselben
Regler anfasst — ein zweiter Agent wie der Mensch an der Oberfläche. Ohne das
wäre der Plan stärker als die Hand daneben.

`musik_cancel` streicht deshalb ohne ausdrückliches `alle=true` nur den einen
genannten Auftrag; eine vergessene `plan_id` leert nicht die Arbeit der
anderen.

## Die Beschreibungen werden erzeugt, nicht gepflegt

Beim Start fragt der Server das laufende Programm, was es kann, und baut daraus
die Beschreibungen von `musik_set` und `musik_do`. Was ein Agent liest, ist
deshalb dasselbe, was in `crates/control/src/katalog.rs` steht — und dasselbe,
was in der Oberfläche als Tooltip erscheint:

```text
Verfügbare Aktionen (erwartetes Argument in Klammern):
  deck1.sync ([deck]) — Auf das andere Deck ziehen — Tempo UND Phase; …
  deck1.load (<pfad>) — Track laden; arbeitet im Hintergrund, …
  deck1.jump_cue (<1..8>) — Einen gesetzten Hot Cue anspringen
```

Ein neues Control erscheint hier ohne eine Zeile Python. Zwei Beschreibungen,
die auseinanderlaufen könnten, gibt es nicht.

Läuft die Anwendung beim Start des Servers nicht, bleibt die Beschreibung
allgemein und verweist auf `musik_list_controls` — die Liste kommt dann zur
Laufzeit.

## Was als Nächstes kommt

Sobald mehr als einer auswählt, ist „was kommt als Nächstes" eine Frage an die
Anlage und nicht an einen Kopf. Zwei Agenten mit je eigener Liste legen
irgendwann beide auf dasselbe Deck; einer verliert, und niemand merkt es, bis
der falsche Track läuft.

```text
musik_queue_add(pfad='…/track.mp3', notiz='passt harmonisch zu 8A, mehr Druck')
musik_queue()          → 1. …/warm.mp3 — ruhig anfangen
                         2. …/track.mp3 — passt harmonisch zu 8A, mehr Druck
musik_queue_next()     → load deck2 angenommen
                         queue 1 abgenommen …/warm.mp3
                         notiz ruhig anfangen
```

**Jeder Eintrag trägt eine Notiz.** Das ist der Unterschied zu einer Playlist:
Wer vormerkt, weiß warum — und ohne die Notiz muss der Nächste den Grund aus
BPM und Tonart erraten. Bei einem Team von Agenten heißt „erraten": erfinden.

Drei Dinge sind mit Absicht so:

- **Derselbe Pfad wird nicht zweimal angenommen.** Die Antwort nennt die
  Nummer, unter der er schon steht. Zwei, die unabhängig nach 128 BPM in 8A
  suchen, finden denselben Track — ihn stillschweigend zweimal aufzunehmen
  hieße, ihn zweimal zu spielen.
- **`musik_queue_next` legt ohne Deckangabe nur auf ein Deck, das nicht
  läuft.** Laufen alle, kommt ein Fehler statt einer Vermutung. Ein Track über
  einen laufenden gelegt reißt den Mix ab.
- **Scheitert das Laden, bleibt der Eintrag stehen.** Sonst wäre er weg, ohne
  gespielt worden zu sein, und das fällt erst auf, wenn er fehlt.

Ändern und Streichen brauchen kein eigenes Werkzeug — `master.queue_drop`,
`queue_bump`, `queue_note` und `queue_clear` nehmen genau ein Argument und
stehen mit ihrer Erklärung schon in der Beschreibung von `musik_do`.

## Sicherheit

- **Kein Netz.** Der Server spricht stdio mit seinem Client und einen
  Unix-Socket mit der Anwendung. Wer die Socketdatei nicht öffnen darf, kommt
  nicht hinein; eine eigene Anmeldung gibt es deshalb nicht.
- **Keine eingeschleusten Befehle.** Das Protokoll ist zeilenweise, also wird
  jedes Argument auf Zeilenumbrüche geprüft und sonst abgewiesen. Ein Pfad mit
  `\n` wäre sonst ein zweiter Befehl, den niemand geschickt hat.
- `musik_do` ist als `destructiveHint` gemeldet: `load` tauscht den Track eines
  Decks, `record` schreibt eine Datei — beides überschreibt etwas. `musik_cancel`
  ebenso: Gestrichenes ist weg und kann von jemand anderem stammen.

## Prüfen

```sh
cargo run --release -p musik-app -- --socket /tmp/musik.sock
MUSIK_SOCKET=/tmp/musik.sock .venv/bin/python test_musik_mcp.py
```

Der Test spricht den Server über einen echten MCP-Client an — `tools/list` und
`tools/call`, nicht an der Schnittstelle vorbei. Ohne laufende Anwendung meldet
er sich mit Rückgabewert 77 ab, statt Grün zu behaupten, wo nichts geprüft
wurde.
