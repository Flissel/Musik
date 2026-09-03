#!/usr/bin/env python3
"""Das Gatter: was ein Agent von sich aus darf.

Die Spezifikation steht in `docs/FREIGABE.md`; hier steht nur, was sie
ausführt. Kurz:

* Klassifiziert wird nach dem, was ein Aufruf **berührt**, nicht nach seinem
  Namen — bei über zweihundert Controls wäre eine Namensliste nach dem nächsten
  Feature falsch.
* Lesen ist frei. Alles andere braucht eine gültige Freigabe.
* Die Freigabe steht in einer **Datei**, nicht in der Umgebung. Eine
  Umgebungsvariable steht fest, sobald der Prozess läuft; wer mitten im Set
  merkt, dass der Agent Unsinn macht, muss ohne Neustart widerrufen können.
* Fail-closed in jedem Zweifelsfall.

**Wogegen das nicht hilft.** Das Gatter schützt davor, dass ein Agent seine
Werkzeuge missbraucht. Es schützt nicht vor etwas, das bereits Socket-Zugang
hat — ein Agent mit einer Shell oder freiem Dateizugriff umgeht es. Dass
solche Werkzeuge im Space nicht freigeschaltet sind, ist Aufgabe dessen, der
ihn einrichtet, und steht dort als erste harte Regel.
"""

from __future__ import annotations

import datetime as _dt
import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

#: Wie lange eine Freigabe höchstens im Voraus gelten darf.
#:
#: Kein Sicherheitsmerkmal, sondern eine Bremse gegen den bequemsten Fehler:
#: einmal „bis 2099" schreiben und nie wieder daran denken.
HOECHSTDAUER = _dt.timedelta(hours=12)

#: Klassen, die es gibt. `lesen` ist immer frei, `erzeugen` nie über die Datei.
KLASSEN = ("lesen", "mischen", "spielen", "zeit", "datei", "dramaturgie", "erzeugen")

#: Klassen, die in der Freigabe-Datei stehen dürfen.
#:
#: `lesen` braucht keine Freigabe, `erzeugen` bekommt keine — es kostet je
#: Aufruf Geld und gehört hinter eine Bestätigung. Beide hier zuzulassen wäre
#: die Art Bequemlichkeit, die man später nicht mehr los wird.
ERTEILBAR = frozenset({"mischen", "spielen", "zeit", "datei", "dramaturgie"})


class FreigabeFehler(RuntimeError):
    """Die Freigabe-Datei ist nicht zu gebrauchen."""


@dataclass(frozen=True)
class Freigabe:
    """Was eine gültige Freigabe-Datei sagt."""

    klassen: frozenset[str]
    bis: _dt.datetime
    von: str

    def gilt(self, jetzt: Optional[_dt.datetime] = None) -> bool:
        jetzt = jetzt or _dt.datetime.now(_dt.timezone.utc)
        return jetzt < self.bis

    def notiz(self) -> str:
        """Die Zeile, die einmal je Fenster in die Mitschrift geht."""
        klassen = " ".join(sorted(self.klassen))
        return f"note freigabe {klassen} bis {self.bis.isoformat()} von {self.von}"


def datei_pfad() -> Optional[Path]:
    roh = os.environ.get("MUSIK_FREIGABE_DATEI", "").strip()
    return Path(roh) if roh else None


def lesen(pfad: Path, jetzt: Optional[_dt.datetime] = None) -> Freigabe:
    """Liest die Freigabe-Datei. Wirft bei allem, was nicht eindeutig ist."""
    jetzt = jetzt or _dt.datetime.now(_dt.timezone.utc)
    try:
        text = pfad.read_text(encoding="utf-8")
    except OSError as fehler:
        raise FreigabeFehler(f"{pfad} nicht lesbar: {fehler}") from fehler

    klassen: Optional[frozenset[str]] = None
    bis: Optional[_dt.datetime] = None
    von = "unbekannt"

    for zeile in text.splitlines():
        zeile = zeile.strip()
        if not zeile or zeile.startswith("#"):
            continue
        wort, _, rest = zeile.partition(" ")
        rest = rest.strip()
        if wort == "klassen":
            gewuenscht = set(rest.split())
            if not gewuenscht:
                raise FreigabeFehler("`klassen` ist leer")
            # Ein Tippfehler verwirft die **ganze** Datei. Sonst funktionierten
            # drei von vier Klassen still weiter, und niemand merkte es.
            unbekannt = gewuenscht - ERTEILBAR
            if unbekannt:
                raise FreigabeFehler(
                    f"unbekannte oder nicht erteilbare Klasse: {' '.join(sorted(unbekannt))}. "
                    f"Erteilbar sind: {' '.join(sorted(ERTEILBAR))}"
                )
            klassen = frozenset(gewuenscht)
        elif wort == "bis":
            bis = _zeitpunkt(rest)
        elif wort == "von":
            von = rest or "unbekannt"

    if klassen is None:
        raise FreigabeFehler("`klassen` fehlt")
    if bis is None:
        raise FreigabeFehler("`bis` fehlt")
    if bis <= jetzt:
        raise FreigabeFehler(f"abgelaufen seit {bis.isoformat()}")
    if bis - jetzt > HOECHSTDAUER:
        raise FreigabeFehler(
            f"`bis` liegt weiter als {int(HOECHSTDAUER.total_seconds() // 3600)} h "
            "voraus — so lange wird nicht im Voraus freigegeben"
        )
    return Freigabe(klassen=klassen, bis=bis, von=von)


def _zeitpunkt(roh: str) -> _dt.datetime:
    """RFC 3339 **mit** Zeitzone.

    Ohne Zeitzone wird abgewiesen und nicht als Ortszeit geraten: Der Prozess
    läuft womöglich woanders als der Mensch, der die Datei geschrieben hat, und
    eine Freigabe, die zwei Stunden länger gilt als gedacht, ist genau das, was
    nicht passieren soll.
    """
    try:
        wert = _dt.datetime.fromisoformat(roh.replace("Z", "+00:00"))
    except ValueError as fehler:
        raise FreigabeFehler(f"`bis {roh}` ist kein Zeitpunkt") from fehler
    if wert.tzinfo is None:
        raise FreigabeFehler(f"`bis {roh}` hat keine Zeitzone — Z oder +02:00 anhängen")
    return wert


def pruefen(klasse: str, befehl: str) -> tuple[bool, str]:
    """Ob dieser Aufruf laufen darf. Gibt (erlaubt, Grund) zurück.

    Gelesen wird bei **jedem** Aufruf. Das kostet einen kurzen Read; gegen einen
    Werkzeugaufruf, der ohnehin über eine Prozessgrenze geht, ist das nichts.
    Zwischenspeichern wäre genau die Optimierung, die das Widerrufen kaputt
    macht.
    """
    if klasse not in KLASSEN:
        return False, f"unbekannte Klasse {klasse!r} — das ist ein Fehler im Werkzeug"
    if klasse == "lesen":
        return True, "lesen ist frei"
    if klasse == "erzeugen":
        return False, (
            "`erzeugen` läuft nie über die Freigabe-Datei — es kostet je Aufruf "
            "Geld und braucht eine Bestätigung je Aufruf"
        )

    pfad = datei_pfad()
    if pfad is None:
        return False, "keine gültige Freigabe (MUSIK_FREIGABE_DATEI nicht gesetzt)"
    try:
        freigabe = lesen(pfad)
    except FreigabeFehler as fehler:
        return False, f"keine gültige Freigabe ({fehler})"
    if klasse not in freigabe.klassen:
        erlaubt = " ".join(sorted(freigabe.klassen))
        return False, f"Freigabe gilt für {erlaubt}, nicht für {klasse}"
    return True, f"freigegeben von {freigabe.von} bis {freigabe.bis.isoformat()}"


def verweigert(klasse: str, befehl: str, grund: str) -> str:
    """Die Antwort bei einer Ablehnung.

    Sie sagt genau, was gelaufen wäre, und wie man es erlauben würde — und
    **nichts über den Zustand der Anlage**. Wer nicht schreiben darf, erfährt
    auch nicht, was gerade läuft.
    """
    if klasse == "erzeugen":
        weg = "eine Bestätigung je Aufruf; die Freigabe-Datei hilft hier nicht"
    else:
        weg = f"`klassen` in MUSIK_FREIGABE_DATEI um {klasse} ergänzen"
    return (
        f"freigabe verweigert für: {befehl}\n"
        f"  Klasse:  {klasse}\n"
        f"  Grund:   {grund}\n"
        f"  Erlaubt: {weg}"
    )


# ── Welche Klasse ein Control berührt ────────────────────────────────────
#
# Die Namen stammen aus `list` an der laufenden Anlage (194 Controls), nicht
# aus dem Gedächtnis. Wer hier etwas ändert, prüft dort nach.

#: Auskunft. Ändert nichts, also frei.
_LESEN = frozenset({
    "artist", "assign", "beat", "beat_phase", "beats_left", "beats_to_outro",
    "beats_to_phrase", "bpm", "duration", "entry", "finished", "intro_beats",
    "key", "key_camelot", "load_status", "phrase_beats", "playlist",
    "playlists", "position", "record_dropped", "record_seconds", "recording",
    "repeats", "section", "section_beats_left", "stems", "title",
    "transitions", "why", "queue", "event_count", "events",
    "arc_actual", "arc_curve", "arc_gap", "arc_minutes", "arc_target",
    "arc_trend", "room_bend",
    "search", "search_harmonic", "search_mixable", "search_next",
})

#: Klang, aber kein neuer Track. Zeitkritisch.
_MISCHEN = frozenset({
    "fader", "trim", "eq_low", "eq_mid", "eq_high", "filter", "cue",
    "crossfader", "crossfader_curve", "gain", "cue_gain", "cue_mix",
    "fx", "fx_amount", "fx_mix", "fx_sync", "fx_time",
})

#: Wiedergabe: was läuft, wo es steht, wie schnell.
_SPIELEN = frozenset({
    "play", "jump_cue", "jump_entry", "sync", "tempo", "keylock",
    "loop_active", "loop_beats", "beatjump", "queue_next",
})

#: Berührt Dateisystem oder Sammlung — nicht zeitkritisch.
_DATEI = frozenset({
    "load", "record", "record_stop", "queue_add",
    "grid_anchor", "grid_here", "grid_scale", "bpm_grid",
})

#: Was das Set vorhat.
_DRAMATURGIE = frozenset({
    "arc", "arc_start", "room", "queue_note", "queue_bump", "queue_clear",
    "queue_drop", "uebergang",
})


def klasse_fuer_control(name: str) -> Optional[str]:
    """Welche Klasse dieses Control berührt, oder `None`.

    `None` heißt **unbekannt** und wird von [`pruefen`] abgewiesen. Das ist
    Absicht: Ein neues Control, das niemand einsortiert hat, soll auffallen und
    nicht stillschweigend in die bequemste Klasse rutschen. Der Preis ist, dass
    jedes neue Control hier einzutragen ist — den zahlt man einmal je Feature,
    und er ist kleiner als eine Freigabe, die mehr erlaubt als gedacht.
    """
    kurz = name.split(".", 1)[-1] if "." in name else name

    # Nummerierte Familien: cue1..8, stem1_level, signal1_trend, …
    if kurz.startswith("cue") and kurz[3:].isdigit():
        return "spielen"
    if kurz.startswith("stem") and len(kurz) > 4:
        rest = kurz[4:]
        if rest[:1].isdigit():
            endung = rest[1:]
            if endung == "_level":
                return "mischen"
            if endung == "_name":
                return "lesen"
            return None
    if kurz.startswith("signal") and len(kurz) > 6:
        rest = kurz[6:]
        if rest[:1].isdigit():
            # signal1 selbst wird gesetzt, signal1_age/_name/_trend gelesen.
            return "dramaturgie" if rest[1:] == "" else "lesen"

    for klasse, namen in (
        ("lesen", _LESEN),
        ("mischen", _MISCHEN),
        ("spielen", _SPIELEN),
        ("datei", _DATEI),
        ("dramaturgie", _DRAMATURGIE),
    ):
        if kurz in namen:
            return klasse
    return None


def pruefen_control(name: str, befehl: str) -> tuple[bool, str]:
    """Wie [`pruefen`], aber die Klasse kommt aus dem Control-Namen."""
    klasse = klasse_fuer_control(name)
    if klasse is None:
        return False, (
            f"unbekanntes Control {name!r} — es ist in mcp/freigabe.py keiner "
            "Klasse zugeordnet. Fail-closed: lieber abweisen als raten."
        )
    return pruefen(klasse, befehl)
