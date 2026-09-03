# Der Space `musik` in VibeMind

Spezifikation. Sie beschreibt, was in `Flissel/Vibemind_V1` angelegt werden
soll, damit Agenten diese Anlage bedienen können.

Stand: Entwurf, September 2026. Nichts davon ist angelegt.

Voraussetzung ist [FREIGABE.md](FREIGABE.md) — ohne das Gatter gehört die
Brücke nicht in einen Space.

## Was dort schon steht, und was daraus folgt

Gelesen in `Vibemind_V1`, nicht aus zweiter Hand:

- **Genau ein Space:** `spaces/marketing/`. Er ist die Vorlage.
- **`mcp/docker/`** ist die Vorlage für einen Server: `server.py`,
  `approval.py`, `pyproject.toml`, `tests/`, `README.md`, `__init__.py`.
- Server importieren `from mcp.server.fastmcp import FastMCP` — das SDK, nicht
  das eigenständige `fastmcp`-Paket. Die Musik-Brücke nutzt heute das andere.
- **`repo-manifest.yaml`** führt größere Komponenten als Submodule mit
  festgehaltenem Pin, Rolle und `migration_state`.
- Der Marketing-Space liegt bewusst **im Repo-Wurzelverzeichnis** und nicht
  unter `vibemind-os/spaces/` — letzteres ist ein Submodul und wird bei
  Submodul-Pulls zurückgesetzt. Eine frühere Fassung ging so verloren.

Daraus folgt der Ablageort ohne weitere Diskussion: `spaces/musik/`, im
Wurzelverzeichnis.

## Die Beweis-Disziplin

Der Marketing-Space trennt streng zwischen **historischem Schnappschuss**,
**konfiguriert/statisch** und **verified_live** und nennt nichts das Dritte
ohne frische, operationsspezifische Evidenz. Diese Disziplin ist zu
übernehmen — sie passt ohnehin zu diesem Projekt, in dem seit dem ersten Tag
gilt, dass eine Zahl ohne Messung keine Zahl ist.

Konkret für `STATUS.md`:

- Jede Behauptung trägt ein Datum und ihre Art.
- „Phase 1 abgenommen" darf erst dastehen, wenn ein Gerät mit vier Ausgängen
  daran hing. Bis dahin: *Code steht, Abnahme offen.*
- Die Testzahl ist statisch (`cargo test --workspace`), nicht live.
- Was nie an echter Musik lief, heißt „an gebautem Material geprüft" und nicht
  „geprüft".

## Aufbau

```text
spaces/musik/
  README.md          Was der Space ist, wie man ihn startet, was er nicht kann
  AGENTS.md          Einstieg für Agenten; Hard Rules zuerst
  STATUS.md          Schnappschuss mit Datum und Evidenzart
  __init__.py
  agents/
    mix_engineer.py      Übergänge planen und fahren
    track_waehler.py     Auswahl aus der Sammlung
    set_planer.py        Dramaturgie über die Zeit
    guardrail.py         Vetorecht
  api/
    __init__.py          Dünne Hülle über die MCP-Werkzeuge
  docs/
    ABNAHME.md           Was geprüft ist, womit, und was fehlt
```

Die Rollen stammen aus [AGENTEN.md](AGENTEN.md). **Crowd-Sensor und
Energie-Analyst fehlen bewusst**: Die Signale haben noch keine Quelle, und ein
Agent, der eine erfindet, ist schlimmer als keiner. Wenn VibeMind einen Kanal
liefert, ist das ein `musik_signal`-Aufruf wie jeder andere.

## Hard Rules für `AGENTS.md`

Nach dem Muster des Marketing-Space: nummeriert, „niemals", und jede mit einem
Test hinterlegt, der sonst rot wird.

1. **Niemals ein Shell- oder Dateiwerkzeug in diesem Space freischalten.** Das
   Freigabe-Gatter sitzt in der Brücke; wer daneben eine Shell hat, umgeht es.
   Diese Regel ist die Voraussetzung dafür, dass die anderen etwas bedeuten.
2. **Niemals die Klasse `erzeugen` in die Freigabe-Datei schreiben.** Generieren
   kostet je Aufruf Geld und gehört hinter eine Bestätigung.
3. **Niemals `musik_set` auf ein Control anwenden, das ein `ramp` besser kann.**
   Ein Sprung am Crossfader ist hörbar; das Repertoire ist dafür da.
4. **Niemals einen Übergang starten, während zwei Decks laufen.** Die Anlage
   weist es ab (`err es laufen mehrere Decks`), und das ist kein Fehler,
   sondern die Antwort.
5. **Niemals `master.arc` mitten im Set neu setzen.** Der Bogen ist das, was
   jemand vorhatte; ihn nachträglich zu ändern macht den Vergleich zwischen
   Plan und Abend wertlos.
6. **Niemals eine Zahl aus `section` oder `energie` als geprüft behandeln.** Die
   Analyse ist nie gegen echte Musik mit gehörter Wahrheit gelaufen.
7. **Niemals länger als eine Phrase auf eine Antwort warten.** Wer nicht
   rechtzeitig fertig ist, lässt den laufenden Track laufen — ein verpasster
   Übergang ist besser als einer im Nichts.

## Der Server: `mcp/musik/`

Umzug der heutigen Brücke, in die dortige Form:

| Heute | Künftig |
| --- | --- |
| `mcp/musik_mcp.py` | `mcp/musik/server.py` |
| — | `mcp/musik/freigabe.py` (aus [FREIGABE.md](FREIGABE.md)) |
| `mcp/test_musik_mcp.py` | `mcp/musik/tests/test_server.py` |
| `mcp/requirements.txt` | `mcp/musik/pyproject.toml` |
| `from fastmcp import FastMCP` | `from mcp.server.fastmcp import FastMCP` |

Die sechzehn Werkzeuge bleiben, was sie sind. **Kein neues Werkzeug für den
Space** — wer für VibeMind etwas hinzufügt, das über den Socket nicht geht, baut
zwei Wege zum selben Ziel, und das ist die Sorte Fehler, die dieses Projekt
bisher vermieden hat.

Zwei Dinge kommen aus der Umgebung:

```json
{
  "env": {
    "MUSIK_SOCKET": "/run/user/1000/musik.sock",
    "MUSIK_FREIGABE_DATEI": "/run/user/1000/musik.freigabe"
  }
}
```

Auf Windows stattdessen `MUSIK_TCP` und `MUSIK_SCHLUESSEL` — VibeMind zielt auf
Windows 11, das ist also der Normalfall und nicht die Ausnahme.

## Der Eintrag im Manifest

Dieses Repo als Submodul, nach dem Muster der bestehenden Einträge:

```yaml
  - id: musik
    parent: vibemind-v1
    path: spaces/musik/engine
    kind: submodule
    remote: https://github.com/Flissel/Musik.git
    remote_role: canonical-origin
    owner: musik
    required: false
```

`required: false`, weil ein VibeMind ohne DJ-Anlage vollständig ist. Der Pin
wird beim Anlegen gesetzt und nicht geraten.

## Reihenfolge und Abnahme

| | Schritt | Fertig, wenn |
| --- | --- | --- |
| 1 | Freigabe-Gatter | die zwölf Tests aus [FREIGABE.md](FREIGABE.md) grün sind |
| 2 | Umzug nach `mcp/musik/` | `tools/list` über einen echten Client dieselben sechzehn Werkzeuge zeigt |
| 3 | `spaces/musik/` mit den drei Dokumenten | `AGENTS.md` die sieben Hard Rules trägt |
| 4 | Manifest-Eintrag | `python -m tools.v1_governance` nicht meckert |
| 5 | Mix-Engineer | er einen Übergang fährt, den `musik-kritik` hinterher wiederfindet |

**Schritt 5 ist der eigentliche Nachweis.** Alles davor ist Verkabelung. Erst
wenn ein Agent einen Übergang gefahren hat und der Kritiker ihn im Mitschnitt
findet — mit Dauer, Phrasenlage und Pegelverlauf —, ist gezeigt, dass die Kette
trägt. Das Werkzeug dafür steht seit S1 und ist an 24 Sets vermessen: Ein
harter Schnitt wird auf ein Fenster genau gefunden, eine Blende über 32 Beats
im Mittel 4,9 s zu spät ([FAHRPLAN.md](FAHRPLAN.md), N3).

## Offene Punkte, die vor Schritt 3 entschieden werden müssen

- **Läuft die Anlage auf demselben Rechner wie VibeMind?** Der Socket setzt das
  voraus. Über Netz ginge nur der Weg über die Rückschleife, und der endet an
  der Maschinengrenze. Ein Agent auf einem anderen Rechner bräuchte einen
  dritten Transport — das ist eine eigene Aufgabe.
- **Wer erteilt die Set-Freigabe im Betrieb?** Ein Mensch am Pult, ein
  Startskript, oder Brain? Die Spezifikation lässt es offen; der Space muss es
  festlegen, sonst legt es sich von selbst fest.
- **Was passiert, wenn der Agent stumm bleibt?** Heute läuft der Track weiter
  und niemand merkt es. Ein Wächter, der nach N Beats ohne Plan meldet, wäre
  klein und sinnvoll — er gehört in den Guardrail.
