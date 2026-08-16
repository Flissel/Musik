#!/usr/bin/env python3
"""MCP-Server für Musik.

Reicht den Steuerraum der laufenden DJ-Anwendung an einen Agenten weiter. Die
eigentliche Arbeit macht `musik-app`; dieser Server ist nur ein Übersetzer
zwischen MCP und dem Zeilenprotokoll auf dem Unix-Socket.

Warum eine eigene Sprache: VibeMind spricht MCP, und ein Agent soll die Anlage
bedienen können, ohne die Oberfläche zu kennen. Warum Python: Genau dort läuft
VibeMind, und die Brücke ist dünn genug, dass sie den Rust-Kern nicht anfasst.

**Der Steuerraum beschreibt sich selbst.** Die Beschreibungen von `musik_set`
und `musik_do` werden beim Start aus dem laufenden Programm erzeugt, nicht von
Hand gepflegt. Ein neues Control in `crates/control/src/katalog.rs` erscheint
damit ohne eine Zeile hier — und die Erklärung, die der Agent liest, ist
dieselbe, die auch als Tooltip in der Oberfläche steht. Zwei Beschreibungen,
die auseinanderlaufen könnten, gibt es nicht.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import tempfile
from enum import Enum
from pathlib import Path
from typing import Any, Optional

from fastmcp import FastMCP
from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

mcp = FastMCP("musik_mcp")

# --------------------------------------------------------------------------
# Verbindung
# --------------------------------------------------------------------------

#: Wie lange auf eine Antwort gewartet wird. Reglerbewegungen sind sofort da;
#: nur `search` fasst eine SQLite-Abfrage an, und auch die ist schnell.
ZEITLIMIT = 5.0

#: Wie viele Signalplätze es gibt. Muss zu `crate::signal::SIGNALE` passen —
#: mehr wäre eine Behauptung über Plätze, die es nicht gibt.
SIGNALE = 4

#: Zeilen, die eine Antwort abschließen. `get` antwortet mit genau einer
#: `value`-Zeile, alles andere endet auf `ok` oder `err`.
ABSCHLUSS = ("ok", "err", "value")


def socket_pfad() -> Path:
    """Wo die Anwendung lauscht.

    `MUSIK_SOCKET` schlägt alles. Sonst das Laufzeitverzeichnis des Benutzers,
    wie es `musik-app` als Standard verwendet.
    """
    if aus_umgebung := os.environ.get("MUSIK_SOCKET"):
        return Path(aus_umgebung)
    if laufzeit := os.environ.get("XDG_RUNTIME_DIR"):
        return Path(laufzeit) / "musik.sock"
    return Path(tempfile.gettempdir()) / "musik.sock"


class NichtErreichbar(RuntimeError):
    """Die Anwendung läuft nicht oder der Socket stimmt nicht."""


def _pruefe_einzeilig(text: str, feld: str) -> str:
    """Weist Zeilenumbrüche ab.

    Das Protokoll ist zeilenweise. Ein Umbruch in einem Dateipfad wäre sonst
    ein zweiter Befehl, den niemand geschickt hat — dieselbe Art Lücke wie eine
    SQL-Injektion, nur billiger zu schließen.
    """
    if "\n" in text or "\r" in text:
        raise ValueError(f"{feld} darf keinen Zeilenumbruch enthalten")
    return text


async def _lies_antwort(leser: asyncio.StreamReader) -> list[str]:
    """Liest die Zeilen bis zum Abschluss einer Antwort."""
    zeilen: list[str] = []
    while True:
        roh = await leser.readline()
        if not roh:
            return zeilen
        zeile = roh.decode("utf-8", "replace").rstrip("\r\n")
        zeilen.append(zeile)
        if zeile.split(" ", 1)[0] in ABSCHLUSS:
            return zeilen


async def sprich(*befehle: str) -> list[list[str]]:
    """Schickt Befehle über eine Verbindung und gibt je eine Antwort zurück.

    Mehrere Befehle in einem Rutsch, weil eine Momentaufnahme sonst ein Dutzend
    Verbindungen kostet.
    """
    pfad = socket_pfad()
    try:
        leser, schreiber = await asyncio.wait_for(
            asyncio.open_unix_connection(str(pfad)), timeout=ZEITLIMIT
        )
    except (FileNotFoundError, ConnectionRefusedError) as fehler:
        raise NichtErreichbar(
            f"Musik ist unter {pfad} nicht erreichbar. Läuft `musik-app`? "
            "Der Socketpfad lässt sich mit der Umgebungsvariablen MUSIK_SOCKET "
            "setzen, in der Anwendung mit --socket."
        ) from fehler
    except asyncio.TimeoutError as fehler:
        raise NichtErreichbar(f"{pfad} antwortet nicht") from fehler

    try:
        antworten: list[list[str]] = []
        for befehl in befehle:
            schreiber.write((befehl + "\n").encode("utf-8"))
            await schreiber.drain()
            antworten.append(await asyncio.wait_for(_lies_antwort(leser), ZEITLIMIT))
        return antworten
    finally:
        schreiber.close()
        try:
            await schreiber.wait_closed()
        except (ConnectionResetError, BrokenPipeError):
            pass


async def eine_antwort(befehl: str) -> list[str]:
    return (await sprich(befehl))[0]


def _fehlerzeile(zeilen: list[str]) -> Optional[str]:
    for zeile in zeilen:
        if zeile.startswith("err "):
            return zeile[4:]
    return None


# --------------------------------------------------------------------------
# Katalog
# --------------------------------------------------------------------------


class Control(BaseModel):
    """Ein Control, so wie das Programm es beschreibt."""

    name: str
    art: str
    raum: str
    einheit: str
    schreibbar: bool
    text: str


def _control_aus_zeile(zeile: str) -> Optional[Control]:
    """`control deck1.tempo zahl 0.92..1.08 faktor rw Tempo-Regler; …`"""
    teile = zeile.split(" ", 6)
    if len(teile) < 7 or teile[0] != "control":
        return None
    return Control(
        name=teile[1],
        art=teile[2],
        raum=teile[3],
        einheit=teile[4],
        schreibbar=teile[5] == "rw",
        text=teile[6],
    )


async def katalog(praefix: str = "") -> list[Control]:
    befehl = f"list {praefix}".strip()
    zeilen = await eine_antwort(befehl)
    if fehler := _fehlerzeile(zeilen):
        raise RuntimeError(fehler)
    return [c for zeile in zeilen if (c := _control_aus_zeile(zeile))]


def _uebersicht(controls: list[Control], nur_aktionen: bool) -> str:
    """Kompakte Aufzählung für eine Tool-Beschreibung."""
    passend = [c for c in controls if (c.art == "aktion") == nur_aktionen]
    if not nur_aktionen:
        passend = [c for c in passend if c.schreibbar]
    return "\n".join(f"  {c.name} ({c.raum}) — {c.text}" for c in passend)


def _startkatalog() -> list[Control]:
    """Holt den Katalog einmal beim Start, für die Tool-Beschreibungen.

    Läuft die Anwendung gerade nicht, bleibt die Beschreibung allgemein — der
    Agent bekommt die richtige Liste dann über `musik_list_controls`.
    """
    try:
        return asyncio.run(katalog())
    except Exception as fehler:  # noqa: BLE001 — beim Start ist alles verzeihlich
        print(f"musik_mcp: Katalog nicht abrufbar ({fehler})", file=sys.stderr)
        return []


KATALOG = _startkatalog()

_HINWEIS_OHNE = (
    "\n\nDie Anwendung lief beim Start dieses Servers nicht, deshalb steht hier "
    "keine Liste. `musik_list_controls` liefert sie zur Laufzeit."
)

BESCHREIBUNG_SET = (
    "Setzt einen Wert im Mischpult — Fader, EQ, Filter, Tempo, Hot Cue.\n\n"
    "Werte außerhalb des erlaubten Bereichs werden begrenzt, nicht abgelehnt. "
    "Zurück kommt der Wert, der wirklich angekommen ist.\n\n"
    + (
        "Schreibbare Controls (Bereich in Klammern):\n" + _uebersicht(KATALOG, False)
        if KATALOG
        else _HINWEIS_OHNE.strip()
    )
)

BESCHREIBUNG_DO = (
    "Löst eine Aktion aus — laden, syncen, einen Hot Cue anspringen, "
    "mitschneiden.\n\n"
    "Aktionen haben keinen Zustand; zum Setzen von Werten ist `musik_set` "
    "zuständig.\n\n"
    + (
        "Verfügbare Aktionen (erwartetes Argument in Klammern):\n"
        + _uebersicht(KATALOG, True)
        if KATALOG
        else _HINWEIS_OHNE.strip()
    )
)


# --------------------------------------------------------------------------
# Eingaben
# --------------------------------------------------------------------------


class Format(str, Enum):
    MARKDOWN = "markdown"
    JSON = "json"


class Basis(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True, extra="forbid")


class ControlEingabe(Basis):
    control: str = Field(
        ...,
        description="Name des Controls, etwa 'deck1.tempo' oder 'channel2.fader'",
        min_length=3,
        max_length=64,
    )

    @field_validator("control")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "control")


class SetzenEingabe(ControlEingabe):
    wert: str = Field(
        ...,
        description=(
            "Neuer Wert. Zahlen als Dezimalzahl ('0.8'), Schalter als '0'/'1', "
            "Auswahlen beim Namen ('delay'), '-' leert einen Hot Cue"
        ),
        min_length=1,
        max_length=256,
    )
    normiert: bool = Field(
        default=False,
        description=(
            "Wert als 0..1 auffassen und in den echten Bereich dehnen — "
            "gedacht für Regler, die ihren Bereich nicht kennen"
        ),
    )

    @field_validator("wert")
    @classmethod
    def _wert_einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "wert")


class AktionEingabe(Basis):
    aktion: str = Field(
        ...,
        description="Name der Aktion, etwa 'deck2.sync' oder 'deck1.load'",
        min_length=3,
        max_length=64,
    )
    argument: Optional[str] = Field(
        default=None,
        description="Argument der Aktion, etwa ein Dateipfad oder eine Zahl",
        max_length=4096,
    )

    @field_validator("aktion", "argument")
    @classmethod
    def _einzeilig(cls, v: Optional[str]) -> Optional[str]:
        return None if v is None else _pruefe_einzeilig(v, "argument")


class ListenEingabe(Basis):
    praefix: str = Field(
        default="",
        description="Nur Controls, deren Name so anfängt, etwa 'deck1.'",
        max_length=64,
    )
    limit: int = Field(default=50, description="Höchstzahl der Treffer", ge=1, le=500)
    offset: int = Field(default=0, description="Wie viele übersprungen werden", ge=0)
    response_format: Format = Field(default=Format.MARKDOWN, description="Ausgabeform")

    @field_validator("praefix")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "praefix")


class SucheEingabe(Basis):
    text: str = Field(
        default="",
        description="Freitext über Titel, Künstler und Album; leer listet alles",
        max_length=256,
    )
    mischbar_zu_bpm: Optional[float] = Field(
        default=None,
        description=(
            "Statt Freitext nach Tempo suchen: Tracks, die zu diesem Wert "
            "passen (±6 %). Schlägt den Freitext."
        ),
        gt=0,
        le=400,
    )
    harmonisch_zu: Optional[str] = Field(
        default=None,
        description=(
            "Statt Freitext nach Tonart suchen: Tracks, deren Tonart harmonisch "
            "passt. Nimmt 'Am', 'F#' oder Camelot '8A'. Schlägt den Freitext, "
            "wird aber selbst von mischbar_zu_bpm geschlagen."
        ),
        max_length=8,
    )
    limit: int = Field(default=25, description="Höchstzahl der Treffer", ge=1, le=200)
    response_format: Format = Field(default=Format.MARKDOWN, description="Ausgabeform")

    @field_validator("text")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "text")

    @field_validator("harmonisch_zu")
    @classmethod
    def _tonart_einzeilig(cls, v: Optional[str]) -> Optional[str]:
        return None if v is None else _pruefe_einzeilig(v, "harmonisch_zu")


class StatusEingabe(Basis):
    response_format: Format = Field(default=Format.MARKDOWN, description="Ausgabeform")


def _zahl_als_text(wert: float) -> str:
    """Zahl fürs Protokoll — ohne Exponentialschreibweise.

    `str(1e-05)` wäre '1e-05', und das Pult liest es zwar, aber in einer
    Protokollzeile sieht es aus wie ein Tippfehler.
    """
    return f"{wert:.6f}".rstrip("0").rstrip(".") or "0"


class DeckName(str, Enum):
    """Die Decks, die es gibt.

    Von Hand aufgezählt und nicht aus dem Katalog erzeugt: Lief die Anwendung
    beim Start nicht, wäre die Auswahl leer und das Schema kaputt. Kommt ein
    drittes Deck dazu, gehört es hierher — das Protokoll selbst nimmt jedes
    `deckN` an.
    """

    DECK1 = "deck1"
    DECK2 = "deck2"


class RampeEingabe(Basis):
    control: str = Field(
        ...,
        description="Regler, der wandern soll, etwa 'channel1.eq_low'",
        min_length=3,
        max_length=64,
    )
    ziel: float = Field(..., description="Wert am Ende der Bewegung")
    ueber_beats: float = Field(
        ...,
        description="Länge der Bewegung in Beats. 0 setzt sofort.",
        ge=0,
        le=4096,
    )
    in_beats: Optional[float] = Field(
        default=None,
        description="Erst so viele Beats warten, dann losfahren",
        ge=0,
        le=4096,
    )
    taktgeber_deck: Optional[DeckName] = Field(
        default=None,
        description=(
            "Wessen Beats gezählt werden. Ohne Angabe erbt ein Kanalzug den "
            "Takt von seinem Deck, die Summe nimmt das erste Deck mit Grid."
        ),
    )

    @field_validator("control")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "control")


class VormerkEingabe(Basis):
    beats: float = Field(
        ...,
        description="In wie vielen Beats der Befehl ausgeführt wird",
        ge=0,
        le=4096,
    )
    aktion: Optional[str] = Field(
        default=None,
        description="Aktion, die dann ausgelöst wird, etwa 'deck2.sync'",
        max_length=64,
    )
    argument: Optional[str] = Field(
        default=None,
        description="Argument der Aktion, etwa ein Dateipfad",
        max_length=4096,
    )
    control: Optional[str] = Field(
        default=None,
        description="Statt einer Aktion: Control, das dann gesetzt wird",
        max_length=64,
    )
    wert: Optional[str] = Field(
        default=None,
        description="Wert für das Control; Pflicht, wenn control gesetzt ist",
        max_length=256,
    )

    @field_validator("aktion", "argument", "control", "wert")
    @classmethod
    def _einzeilig(cls, v: Optional[str]) -> Optional[str]:
        return None if v is None else _pruefe_einzeilig(v, "Argument")

    @model_validator(mode="after")
    def _genau_eines(self) -> VormerkEingabe:
        if bool(self.aktion) == bool(self.control):
            raise ValueError("entweder aktion oder control angeben, nicht beides")
        if self.control and not self.wert:
            raise ValueError("control braucht einen wert")
        return self

    def befehl(self) -> str:
        if self.aktion:
            return f"do {self.aktion}" + (f" {self.argument}" if self.argument else "")
        return f"set {self.control} {self.wert}"


class StreichEingabe(Basis):
    plan_id: Optional[int] = Field(
        default=None,
        description="Nummer aus `musik_status` — der eine Auftrag, der weg soll",
        ge=1,
    )
    alle: bool = Field(
        default=False,
        description=(
            "Alles streichen, auch was andere vorgemerkt haben. Bewusst ein "
            "eigener Schalter, damit ein vergessenes plan_id nicht den ganzen "
            "Plan leert."
        ),
    )

    @model_validator(mode="after")
    def _eines_von_beiden(self) -> StreichEingabe:
        if (self.plan_id is None) == (not self.alle):
            raise ValueError("entweder plan_id nennen oder alle=true setzen")
        return self


class Richtung(str, Enum):
    UNTER = "unter"
    UEBER = "ueber"


class BedingungEingabe(Basis):
    control: str = Field(
        ...,
        description="Wert, auf den gewartet wird, etwa 'deck1.beats_left'",
        min_length=3,
        max_length=64,
    )
    richtung: Richtung = Field(
        ...,
        description="'unter' feuert, sobald der Wert die Schwelle unterschreitet",
    )
    schwelle: float = Field(..., description="Die Schwelle")
    aktion: Optional[str] = Field(
        default=None,
        description="Aktion, die dann ausgelöst wird, etwa 'master.queue_next'",
        max_length=64,
    )
    argument: Optional[str] = Field(
        default=None, description="Argument der Aktion", max_length=4096
    )
    control_setzen: Optional[str] = Field(
        default=None,
        description="Statt einer Aktion: Control, das dann gesetzt wird",
        max_length=64,
    )
    wert: Optional[str] = Field(
        default=None,
        description="Wert dafür; Pflicht, wenn control_setzen gesetzt ist",
        max_length=256,
    )

    @field_validator("control", "aktion", "argument", "control_setzen", "wert")
    @classmethod
    def _einzeilig(cls, v: Optional[str]) -> Optional[str]:
        return None if v is None else _pruefe_einzeilig(v, "Argument")

    @model_validator(mode="after")
    def _genau_eines(self) -> BedingungEingabe:
        if bool(self.aktion) == bool(self.control_setzen):
            raise ValueError("entweder aktion oder control_setzen angeben, nicht beides")
        if self.control_setzen and not self.wert:
            raise ValueError("control_setzen braucht einen wert")
        return self

    def befehl(self) -> str:
        if self.aktion:
            return f"do {self.aktion}" + (f" {self.argument}" if self.argument else "")
        return f"set {self.control_setzen} {self.wert}"


class SignalEingabe(Basis):
    name: str = Field(
        ...,
        description=(
            "Wofür das Signal steht, etwa 'Energie auf der Flaeche'. Derselbe "
            "Name landet immer auf demselben Platz."
        ),
        min_length=1,
        max_length=64,
    )
    wert: float = Field(
        ...,
        description="Der Messwert, -1 bis 1. 0 ist neutral, nicht 'nichts'.",
        ge=-1,
        le=1,
    )

    @field_validator("name")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "name")


class ListeEingabe(Basis):
    limit: int = Field(default=50, description="Höchstzahl der Einträge", ge=1, le=200)
    response_format: Format = Field(default=Format.MARKDOWN, description="Ausgabeform")


class VormerkenEingabe(Basis):
    pfad: str = Field(
        ...,
        description="Dateipfad des Tracks, so wie ihn `musik_search` liefert",
        min_length=1,
        max_length=4096,
    )
    notiz: str = Field(
        default="",
        description=(
            "Warum er dort steht — 'passt harmonisch zu 8A', 'mehr Druck nach "
            "dem Break'. Der Nächste, der die Liste liest, muss den Grund sonst "
            "erraten."
        ),
        max_length=512,
    )
    als_naechstes: bool = Field(
        default=False,
        description="Direkt an den Anfang statt hinten anhängen",
    )

    @field_validator("pfad", "notiz")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "Argument")


class AuflegenEingabe(Basis):
    deck: Optional[DeckName] = Field(
        default=None,
        description=(
            "Auf welches Deck. Ohne Angabe auf eines, das gerade nicht läuft; "
            "laufen alle, kommt ein Fehler statt eines abgerissenen Mixes."
        ),
    )


# --------------------------------------------------------------------------
# Werkzeuge
# --------------------------------------------------------------------------

NUR_LESEN = {
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


@mcp.tool(
    name="musik_status",
    annotations={"title": "Momentaufnahme der Anlage", **NUR_LESEN},
)
async def musik_status(params: StatusEingabe) -> str:
    """Was gerade läuft: beide Decks, die Kanalzüge, die Summe — und der Plan.

    Der erste Griff, bevor man etwas verändert. Eine Momentaufnahme statt eines
    Dutzends einzelner Abfragen.

    **Der Plan steht mit drin, weil selten nur einer bedient.** Wer sieht, dass
    schon eine Blende auf `channel1.fader` läuft, greift nicht mitten hinein —
    und wenn doch, bricht die Blende ab (die Anlage bevorzugt den letzten
    Griff, nicht den Plan).

    Args:
        params (StatusEingabe): response_format — 'markdown' oder 'json'.

    Returns:
        str: Markdown-Übersicht oder JSON mit dieser Form:
        {
          "decks": [{"deck": "deck1", "title": str, "artist": str,
                     "bpm": float|null, "key": str|null, "key_camelot": str|null,
                     "position": float, "duration": float,
                     "beat": float|null, "beats_left": float|null,
                     "beats_to_phrase": float|null, "phrase_beats": float|null,
                     "playing": bool, "finished": bool, "load_status": str}],
          "channels": [{"channel": "channel1", "fader": float, "cue": bool,
                        "fx": str}],
          "master": {"crossfader": float, "gain": float, "recording": bool,
                     "record_seconds": float, "record_dropped": float},
          "signals": [{"slot": int, "name": str, "value": float|null,
                       "trend_per_minute": float|null, "age_seconds": float|null}],
          "plan": [{"id": int, "art": "ramp"|"in"|"wenn", "text": str}]
        }

    Beispiele:
        - „Was liegt auf den Decks?"
        - „Läuft die Aufnahme noch?"
        - „Wie lange habe ich noch?" → `beats_left`, in Grid-Beats gezählt.
        - „Hat jemand schon etwas vorgemerkt?" → das Feld `plan`.
        - Nicht dafür: einen einzelnen Wert lesen → `musik_get`.
    """
    try:
        return await _status(params.response_format)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"


def _als_zahl(text: str) -> Optional[float]:
    try:
        return float(text)
    except ValueError:
        return None


def _als_text(text: str) -> Optional[str]:
    """`-` ist die Antwort des Pults für „leer" — die wird nicht weitergereicht.

    Sonst stünde in der Ausgabe ein Bindestrich, wo „unbekannt" gemeint ist,
    und der ließe sich nicht von einem echten Wert unterscheiden.
    """
    return None if text in ("-", "") else text


async def _werte(controls: list[str]) -> dict[str, str]:
    """Liest mehrere Controls über eine Verbindung."""
    antworten = await sprich(*(f"get {c}" for c in controls))
    aus: dict[str, str] = {}
    for control, zeilen in zip(controls, antworten):
        zeile = zeilen[-1] if zeilen else ""
        aus[control] = zeile.split(" ", 2)[2] if zeile.startswith("value ") else "-"
    return aus


def _liste_aus_zeilen(zeilen: list[str]) -> list[dict[str, Any]]:
    """`queue 3 /musik/track.mp3 mehr Druck nach dem Break`

    Pfad und Notiz werden an der Dateiendung getrennt — dieselbe Regel wie bei
    den Suchtreffern, damit beides Leerzeichen enthalten darf.
    """
    aus: list[dict[str, Any]] = []
    for zeile in zeilen:
        if not zeile.startswith("queue "):
            continue
        teile = zeile.split(" ", 2)
        if len(teile) < 3 or not teile[1].isdigit():
            continue
        pfad, notiz = _pfad_und_titel(teile[2])
        aus.append({"nr": int(teile[1]), "path": pfad, "note": None if notiz == "-" else notiz})
    return aus


def _plan_aus_zeilen(zeilen: list[str]) -> list[dict[str, Any]]:
    """`plan 1 ramp channel1.fader 0.0000 → 1.0000 über 16 Beats, …`"""
    aus: list[dict[str, Any]] = []
    for zeile in zeilen:
        if not zeile.startswith("plan "):
            continue
        teile = zeile.split(" ", 2)
        if len(teile) < 3 or not teile[1].isdigit():
            continue
        aus.append(
            {
                "id": int(teile[1]),
                "art": teile[2].split(" ", 1)[0],
                "text": teile[2],
            }
        )
    return aus


async def _status(form: Format) -> str:
    vorhandene = {c.name for c in await katalog()}
    decks = sorted({n.split(".")[0] for n in vorhandene if n.startswith("deck")})
    kanaele = sorted({n.split(".")[0] for n in vorhandene if n.startswith("channel")})

    deck_felder = [
        "title",
        "artist",
        "bpm",
        "key",
        "key_camelot",
        "position",
        "duration",
        # Die musikalischen Größen gehören in dieselbe Momentaufnahme wie alles
        # andere. Wer sie sich aus Position, Länge und Tempo selbst ausrechnet,
        # rechnet sie bei jedem Blick neu — und macht dabei irgendwann einen
        # Fehler, den niemand sieht.
        "beat",
        "beats_left",
        "beats_to_phrase",
        "phrase_beats",
        "play",
        "finished",
        "load_status",
    ]
    kanal_felder = ["fader", "cue", "fx"]
    signal_felder = [
        f"master.signal{i}{f}"
        for i in range(1, SIGNALE + 1)
        for f in ("_name", "", "_trend", "_age")
    ]
    master_felder = [
        "crossfader",
        "gain",
        "recording",
        "record_seconds",
        "record_dropped",
    ]

    gefragt = (
        [f"{d}.{f}" for d in decks for f in deck_felder]
        + [f"{k}.{f}" for k in kanaele for f in kanal_felder]
        + [f"master.{f}" for f in master_felder]
        + signal_felder
    )
    roh = await _werte(gefragt)
    zeitplan, liste = await sprich("plan", "do master.queue")
    plan = _plan_aus_zeilen(zeitplan)
    warteschlange = _liste_aus_zeilen(liste)

    daten: dict[str, Any] = {
        "decks": [
            {
                "deck": d,
                "title": roh.get(f"{d}.title", ""),
                "artist": roh.get(f"{d}.artist", ""),
                "bpm": _als_zahl(roh.get(f"{d}.bpm", "-")),
                "key": _als_text(roh.get(f"{d}.key", "-")),
                "key_camelot": _als_text(roh.get(f"{d}.key_camelot", "-")),
                "position": _als_zahl(roh.get(f"{d}.position", "-")) or 0.0,
                "duration": _als_zahl(roh.get(f"{d}.duration", "-")) or 0.0,
                "beat": _als_zahl(roh.get(f"{d}.beat", "-")),
                "beats_left": _als_zahl(roh.get(f"{d}.beats_left", "-")),
                "beats_to_phrase": _als_zahl(roh.get(f"{d}.beats_to_phrase", "-")),
                "phrase_beats": _als_zahl(roh.get(f"{d}.phrase_beats", "-")),
                "playing": roh.get(f"{d}.play") == "1",
                "finished": roh.get(f"{d}.finished") == "1",
                "load_status": roh.get(f"{d}.load_status", ""),
            }
            for d in decks
        ],
        "channels": [
            {
                "channel": k,
                "fader": _als_zahl(roh.get(f"{k}.fader", "-")) or 0.0,
                "cue": roh.get(f"{k}.cue") == "1",
                "fx": roh.get(f"{k}.fx", "off"),
            }
            for k in kanaele
        ],
        "master": {
            "crossfader": _als_zahl(roh.get("master.crossfader", "-")) or 0.0,
            "gain": _als_zahl(roh.get("master.gain", "-")) or 0.0,
            "recording": roh.get("master.recording") == "1",
            "record_seconds": _als_zahl(roh.get("master.record_seconds", "-")) or 0.0,
            "record_dropped": _als_zahl(roh.get("master.record_dropped", "-")) or 0.0,
        },
        # Nur die benutzten Plätze: Vier leere Zeilen zu melden hieße, dem
        # Leser vier Dinge zu zeigen, über die niemand etwas gesagt hat.
        "signals": [
            {
                "slot": i,
                "name": roh.get(f"master.signal{i}_name", ""),
                "value": _als_zahl(roh.get(f"master.signal{i}", "-")),
                "trend_per_minute": _als_zahl(roh.get(f"master.signal{i}_trend", "-")),
                "age_seconds": _als_zahl(roh.get(f"master.signal{i}_age", "-")),
            }
            for i in range(1, SIGNALE + 1)
            if _als_text(roh.get(f"master.signal{i}_name", "-"))
        ],
        "plan": plan,
        "queue": {
            "count": len(warteschlange),
            "next": warteschlange[0] if warteschlange else None,
        },
    }

    if form is Format.JSON:
        return json.dumps(daten, indent=2, ensure_ascii=False)

    zeilen = ["# Musik"]
    for d in daten["decks"]:
        tempo = f"{d['bpm']:.2f} BPM" if d["bpm"] else "kein Beatgrid"
        lauf = "läuft" if d["playing"] else "steht"
        zeilen.append(
            f"\n## {d['deck'].upper()} — {d['title'] or 'nichts geladen'}"
            + (f" ({d['artist']})" if d["artist"] else "")
        )
        zeilen.append(
            f"- {tempo}, {lauf} bei {d['position']:.1f} s von {d['duration']:.1f} s"
        )
        if d["beats_left"] is not None:
            zeilen.append(
                f"- noch **{d['beats_left']:.0f} Beats**, "
                f"{d['beats_to_phrase']:.0f} bis zur Phrasengrenze"
            )
        if d["key"]:
            zeilen.append(f"- Tonart {d['key']} ({d['key_camelot']})")
        if d["finished"]:
            zeilen.append("- **durchgelaufen**")
        if d["load_status"] not in ("bereit", ""):
            zeilen.append(f"- Laden: {d['load_status']}")

    zeilen.append("\n## Mischpult")
    for k in daten["channels"]:
        extra = f", FX {k['fx']}" if k["fx"] not in ("off", "-") else ""
        cue = ", Kopfhörer" if k["cue"] else ""
        zeilen.append(f"- {k['channel']}: Fader {k['fader']:.2f}{extra}{cue}")

    m = daten["master"]
    zeilen.append(
        f"- Crossfader {m['crossfader']:+.2f}, Summe {m['gain']:.2f}"
    )
    if m["recording"]:
        warnung = (
            f" — **{m['record_dropped']:.0f} Frames fehlen**"
            if m["record_dropped"]
            else ""
        )
        zeilen.append(f"- Mitschnitt läuft: {m['record_seconds']:.1f} s{warnung}")

    if daten["signals"]:
        zeilen.append("\n## Aus dem Raum")
        for s in daten["signals"]:
            wert = "—" if s["value"] is None else f"{s['value']:+.2f}"
            trend = (
                ""
                if s["trend_per_minute"] is None
                else f", {s['trend_per_minute']:+.2f}/min"
            )
            # Das Alter gehört dazu: Ein Wert von vor zwanzig Minuten ist keine
            # Lüge, aber auch keine Auskunft über jetzt.
            alt = (
                f" (vor {s['age_seconds']:.0f} s)"
                if s["age_seconds"] is not None and s["age_seconds"] > 120
                else ""
            )
            zeilen.append(f"- **{s['name']}** {wert}{trend}{alt}")

    if plan:
        zeilen.append("\n## Vorgemerkt")
        zeilen += [f"- **{a['id']}** {a['text']}" for a in plan]

    if warteschlange:
        naechster = warteschlange[0]
        zeilen.append(
            f"\n## Liste ({len(warteschlange)})\n"
            f"- als Nächstes **{naechster['nr']}** `{naechster['path']}`"
            + (f" — {naechster['note']}" if naechster["note"] else "")
        )

    return "\n".join(zeilen)


@mcp.tool(
    name="musik_list_controls",
    annotations={"title": "Steuerraum aufzählen", **NUR_LESEN},
)
async def musik_list_controls(params: ListenEingabe) -> str:
    """Zählt auf, was sich bedienen lässt — mit Bereich, Einheit und Bedeutung.

    Der Steuerraum beschreibt sich selbst: Jedes Control nennt seinen Typ, den
    erlaubten Wertebereich, die Einheit, ob es schreibbar ist und was es tut.
    Wer das gelesen hat, braucht kein Handbuch.

    Args:
        params (ListenEingabe):
            - praefix (str): nur Namen, die so anfangen, etwa 'deck1.'
            - limit (int): 1–500, Standard 50
            - offset (int): zum Blättern
            - response_format: 'markdown' oder 'json'

    Returns:
        str: Markdown-Tabelle oder JSON:
        {"total": int, "count": int, "offset": int, "has_more": bool,
         "next_offset": int|null,
         "controls": [{"name": str, "art": str, "raum": str, "einheit": str,
                       "schreibbar": bool, "text": str}]}

    Beispiele:
        - „Was kann Deck 1?" → praefix='deck1.'
        - „Welche Effekte gibt es?" → praefix='channel1.fx'
        - Nicht dafür: aktuelle Werte lesen → `musik_get` oder `musik_status`.
    """
    try:
        alle = await katalog(params.praefix)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"
    except RuntimeError as fehler:
        return f"Fehler: {fehler}"

    ausschnitt = alle[params.offset : params.offset + params.limit]
    weiter = params.offset + len(ausschnitt) < len(alle)

    if params.response_format is Format.JSON:
        return json.dumps(
            {
                "total": len(alle),
                "count": len(ausschnitt),
                "offset": params.offset,
                "has_more": weiter,
                "next_offset": params.offset + len(ausschnitt) if weiter else None,
                "controls": [c.model_dump() for c in ausschnitt],
            },
            indent=2,
            ensure_ascii=False,
        )

    if not ausschnitt:
        return f"Keine Controls mit dem Anfang '{params.praefix}'."

    zeilen = [
        f"# Steuerraum ({len(ausschnitt)} von {len(alle)})",
        "",
        "| Control | Typ | Bereich | Einheit | | Bedeutung |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    zeilen += [
        f"| `{c.name}` | {c.art} | {c.raum} | {c.einheit} | "
        f"{'rw' if c.schreibbar else 'r'} | {c.text} |"
        for c in ausschnitt
    ]
    if weiter:
        zeilen.append(f"\nWeitere ab offset={params.offset + len(ausschnitt)}.")
    return "\n".join(zeilen)


@mcp.tool(
    name="musik_get",
    annotations={"title": "Einen Wert lesen", **NUR_LESEN},
)
async def musik_get(params: ControlEingabe) -> str:
    """Liest einen einzelnen Wert.

    Args:
        params (ControlEingabe): control (str) — etwa 'deck1.bpm'.

    Returns:
        str: Der Wert als Text. '-' heißt, dass es ihn gerade nicht gibt —
        ein ungesetzter Hot Cue, ein Deck ohne Beatgrid. Bei einem Fehler
        'Fehler: <Grund>'.

    Beispiele:
        - „Wie schnell läuft Deck 2?" → control='deck2.bpm'
        - Nicht dafür: viele Werte auf einmal → `musik_status`.
    """
    try:
        zeilen = await eine_antwort(f"get {params.control}")
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}. `musik_list_controls` zeigt, was es gibt."
    zeile = zeilen[-1] if zeilen else ""
    return zeile.split(" ", 2)[2] if zeile.startswith("value ") else "-"


@mcp.tool(
    name="musik_search",
    annotations={"title": "Sammlung durchsuchen", **NUR_LESEN},
)
async def musik_search(params: SucheEingabe) -> str:
    """Sucht Tracks in der Sammlung — nach Text, Tempo oder Tonart.

    Args:
        params (SucheEingabe):
            - text (str): Freitext über Titel, Künstler, Album
            - mischbar_zu_bpm (float|None): stattdessen nach Tempo suchen (±6 %)
            - harmonisch_zu (str|None): stattdessen nach Tonart suchen
            - limit (int): 1–200, Standard 25
            - response_format: 'markdown' oder 'json'

    Returns:
        str: Markdown-Liste oder JSON:
        {"count": int, "truncated": bool,
         "tracks": [{"bpm": float|null, "key": str|null, "path": str,
                     "title": str}]}
        `key` ist die Camelot-Zahl (8A, 5B); `path` ist es, was `musik_do` mit
        der Aktion 'deckN.load' braucht.

    Beispiele:
        - „Was habe ich von Alpenglühen?" → text='Alpen'
        - „Was passt zu 128 BPM?" → mischbar_zu_bpm=128
        - „Was passt harmonisch zu Deck 1?" → erst `musik_get('deck1.key')`,
          dann harmonisch_zu mit dem Ergebnis.
        - Nicht dafür: einen Track auflegen → `musik_do('deck1.load', pfad)`.
    """
    if params.mischbar_zu_bpm is not None:
        befehl = f"do master.search_mixable {params.mischbar_zu_bpm}"
    elif params.harmonisch_zu:
        befehl = f"do master.search_harmonic {params.harmonisch_zu}"
    else:
        befehl = f"do master.search {params.text}".rstrip()

    try:
        zeilen = await eine_antwort(befehl)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"
    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}"

    treffer: list[dict[str, Any]] = []
    begrenzt = False
    for zeile in zeilen:
        if zeile.startswith("hinweis "):
            begrenzt = True
            continue
        if not zeile.startswith("track "):
            continue
        teile = zeile.split(" ", 3)
        if len(teile) < 4:
            continue
        # `track <bpm> <key> <pfad> <titel>` — der Pfad kann Leerzeichen
        # enthalten, der Titel auch. Getrennt wird deshalb hinten am Titel, der
        # immer der Dateiname ohne Endung oder der Tag ist.
        pfad, titel = _pfad_und_titel(teile[3])
        treffer.append(
            {
                "bpm": _als_zahl(teile[1]),
                "key": None if teile[2] == "-" else teile[2],
                "path": pfad,
                "title": titel,
            }
        )

    treffer = treffer[: params.limit]
    if params.response_format is Format.JSON:
        return json.dumps(
            {"count": len(treffer), "truncated": begrenzt, "tracks": treffer},
            indent=2,
            ensure_ascii=False,
        )

    if not treffer:
        return "Keine Treffer."
    zeilen_aus = [f"# {len(treffer)} Treffer", ""]
    for t in treffer:
        tempo = f"{t['bpm']:.2f} BPM" if t["bpm"] else "kein Tempo"
        key = f", {t['key']}" if t["key"] else ""
        zeilen_aus.append(f"- **{t['title']}** — {tempo}{key}\n  `{t['path']}`")
    if begrenzt:
        zeilen_aus.append("\nDie Liste ist begrenzt; es gibt mehr.")
    return "\n".join(zeilen_aus)


def _pfad_und_titel(rest: str) -> tuple[str, str]:
    """Trennt `<pfad> <titel>` — beide dürfen Leerzeichen enthalten.

    Der Titel ist der Dateiname ohne Endung, wenn es keine Tags gibt. Also
    wird von hinten gesucht: Der Pfad endet an der Endung.
    """
    for endung in (".wav", ".mp3", ".flac", ".m4a", ".ogg", ".aiff", ".aif"):
        stelle = rest.lower().find(endung + " ")
        if stelle != -1:
            schnitt = stelle + len(endung)
            return rest[:schnitt], rest[schnitt:].strip()
    pfad, _, titel = rest.partition(" ")
    return pfad, titel


@mcp.tool(
    name="musik_set",
    description=BESCHREIBUNG_SET,
    annotations={
        "title": "Einen Wert setzen",
        "readOnlyHint": False,
        "destructiveHint": False,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def musik_set(params: SetzenEingabe) -> str:
    """Setzt einen Wert. Ausführliche Beschreibung siehe `description`.

    Args:
        params (SetzenEingabe): control, wert, normiert.

    Returns:
        str: 'ok <control> <wert>' mit dem tatsächlich angekommenen Wert, oder
        'Fehler: <Grund>'.
    """
    verb = "setn" if params.normiert else "set"
    try:
        zeilen = await eine_antwort(f"{verb} {params.control} {params.wert}")
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}. `musik_list_controls` zeigt, was es gibt."
    return "\n".join(zeilen)


@mcp.tool(
    name="musik_do",
    description=BESCHREIBUNG_DO,
    annotations={
        "title": "Eine Aktion auslösen",
        "readOnlyHint": False,
        # `load` tauscht den Track eines Decks, `record` schreibt eine Datei —
        # beides überschreibt etwas, das vorher da war.
        "destructiveHint": True,
        "idempotentHint": False,
        "openWorldHint": False,
    },
)
async def musik_do(params: AktionEingabe) -> str:
    """Löst eine Aktion aus. Ausführliche Beschreibung siehe `description`.

    Args:
        params (AktionEingabe): aktion, argument.

    Returns:
        str: Die Antwortzeilen des Programms. `sync` nennt Tempo und
        Phasenfehler, `load` meldet Annahme (nicht Erledigung — der Fortschritt
        steht in 'deckN.load_status'), `search` gibt Treffer aus.
        Bei einem Fehler 'Fehler: <Grund>'.
    """
    befehl = f"do {params.aktion}"
    if params.argument:
        befehl += f" {params.argument}"

    try:
        zeilen = await eine_antwort(befehl)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}. `musik_list_controls` zeigt die Aktionen."
    return "\n".join(zeilen)


# --------------------------------------------------------------------------
# Zeit
# --------------------------------------------------------------------------
#
# Warum das nicht der Agent selbst macht: Ein Übergang ist eine Bewegung über
# Takte, keine Folge von Reglerstellungen. Wer ihn von außen nachbaut, müsste
# `musik_set` in einer engen Schleife rufen und dazwischen schlafen — über eine
# Werkzeugschnittstelle, deren Timing bei jedem Aufruf eine Modellantwort weit
# entfernt ist. Das eiert hörbar, und der Agent kann in der Zeit nichts anderes
# tun. Hier sagt er einmal, was passieren soll, und ist wieder frei.

SCHREIBT = {
    "readOnlyHint": False,
    "destructiveHint": False,
    "idempotentHint": False,
    "openWorldHint": False,
}


@mcp.tool(
    name="musik_ramp",
    annotations={"title": "Einen Regler über Beats bewegen", **SCHREIBT},
)
async def musik_ramp(params: RampeEingabe) -> str:
    """Fährt einen Regler über mehrere Beats auf einen Wert — eine Blende.

    Gerechnet wird in **Beats, nicht in Sekunden**: Dreht jemand am Tempo,
    bleibt die Blende musikalisch richtig. Steht das Deck, wartet sie.

    Der Aufruf kommt sofort zurück; die Bewegung läuft in der Anwendung weiter.
    Wie weit sie ist, steht im Feld `plan` von `musik_status`.

    **Sie gibt auf, sobald jemand anders denselben Regler anfasst** — ein
    Mensch an der Oberfläche wie ein zweiter Agent. Wer also mitten in eine
    fremde Blende `musik_set` ruft, gewinnt und beendet sie damit.

    Args:
        params (RampeEingabe):
            - control (str): der Regler, etwa 'channel1.eq_low'. Muss eine Zahl
              sein — ein Schalter lässt sich nicht blenden.
            - ziel (float): der Wert am Ende
            - ueber_beats (float): Länge der Bewegung
            - in_beats (float|None): erst so lange warten
            - taktgeber_deck ('deck1'|'deck2'|None): wessen Beats gezählt werden

    Returns:
        str: 'ok plan <nr> …' mit der Nummer, unter der der Auftrag im Plan
        steht — die braucht `musik_cancel`. Sonst 'Fehler: <Grund>'.

    Beispiele:
        - „Blende den Bass von Deck 1 über 8 Beats raus" →
          control='channel1.eq_low', ziel=0, ueber_beats=8
        - „Nach 16 Beats den Crossfader über 32 rüberziehen" →
          control='master.crossfader', ziel=1, ueber_beats=32, in_beats=16
        - Nicht dafür: einen Wert sofort setzen → `musik_set`.
    """
    befehl = (
        f"ramp {params.control} {_zahl_als_text(params.ziel)} "
        f"{_zahl_als_text(params.ueber_beats)}"
    )
    if params.taktgeber_deck:
        befehl += f" {params.taktgeber_deck.value}"
    if params.in_beats is not None:
        befehl = f"in {_zahl_als_text(params.in_beats)} {befehl}"

    try:
        zeilen = await eine_antwort(befehl)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}"
    return "\n".join(zeilen)


@mcp.tool(
    name="musik_schedule",
    annotations={"title": "Etwas auf einen Beat legen", **SCHREIBT},
)
async def musik_schedule(params: VormerkEingabe) -> str:
    """Merkt eine Aktion oder einen Wert für einen späteren Beat vor.

    Für alles, was auf den Takt gehört statt auf die Uhr: einen zweiten Track
    auf der 33 starten, nach 64 Beats den Kanal aufziehen, am Ende der Phrase
    syncen.

    Args:
        params (VormerkEingabe):
            - beats (float): in wie vielen Beats
            - **entweder** aktion (str) + argument (str|None)
            - **oder** control (str) + wert (str)

    Returns:
        str: 'ok plan <nr> …' mit der Nummer für `musik_cancel`, sonst
        'Fehler: <Grund>'. Was der Befehl dann geantwortet hat, sieht ein
        Abonnent auf dem Socket; über MCP zeigt es sich am Zustand.

    Beispiele:
        - „Starte Deck 2 in 16 Beats" → beats=16, control='deck2.play', wert='1'
        - „Sync in einer Phrase" → beats=32, aktion='deck2.sync'
        - Nicht dafür: eine Blende → `musik_ramp` (auch verzögert, mit
          in_beats).
    """
    try:
        zeilen = await eine_antwort(
            f"in {_zahl_als_text(params.beats)} {params.befehl()}"
        )
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}. `musik_list_controls` zeigt, was es gibt."
    return "\n".join(zeilen)


@mcp.tool(
    name="musik_cancel",
    annotations={
        "title": "Vorgemerktes zurücknehmen",
        "readOnlyHint": False,
        # Was gestrichen ist, ist weg — und kann von jemand anderem stammen.
        "destructiveHint": True,
        "idempotentHint": True,
        "openWorldHint": False,
    },
)
async def musik_cancel(params: StreichEingabe) -> str:
    """Nimmt einen vorgemerkten Auftrag zurück.

    Die Nummern stehen im Feld `plan` von `musik_status`. **Der Plan ist
    gemeinsam** — was dort steht, kann ein anderer Agent oder der Mensch an der
    Oberfläche vorgemerkt haben. Deshalb streicht dieses Werkzeug ohne
    ausdrückliches `alle=true` nur den einen genannten Auftrag.

    Eine laufende Blende bleibt stehen, wo sie gerade ist; sie fährt nicht
    zurück. Wer den Ausgangswert zurückhaben will, setzt ihn selbst.

    Args:
        params (StreichEingabe): plan_id (int|None), alle (bool).

    Returns:
        str: 'ok <n> gestrichen' oder 'Fehler: <Grund>'.

    Beispiele:
        - „Nimm die Blende zurück" → erst `musik_status`, dann plan_id=<nr>
        - „Alles abbrechen" → alle=true
    """
    befehl = "cancel" if params.alle else f"cancel {params.plan_id}"
    try:
        zeilen = await eine_antwort(befehl)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}"
    return "\n".join(zeilen)


@mcp.tool(
    name="musik_signal",
    annotations={
        "title": "Etwas von außen melden",
        "readOnlyHint": False,
        "destructiveHint": False,
        # Zwei Meldungen desselben Namens sind zwei Messpunkte, keine
        # Wiederholung — daraus entsteht ja gerade der Trend.
        "idempotentHint": False,
        "openWorldHint": False,
    },
)
async def musik_signal(params: SignalEingabe) -> str:
    """Meldet einen Wert aus dem Raum — Energie, Andrang, Stimmung.

    Ein DJ liest die Fläche. Ein Agent kann das nicht sehen, also muss es
    jemand hereingeben: ein Mikrofonpegel, eine Umfrage, ein Mensch im Chat.
    Ab dann ist es ein Control wie jedes andere und lässt sich mit `musik_when`
    zur Bedingung machen.

    **Ein einzelner Wert nützt wenig.** „Energie 0,7" beantwortet keine Frage;
    „0,7 und seit zwei Minuten fallend" beantwortet sie. Deshalb wird jede
    Meldung als Messpunkt aufbewahrt und der Trend daraus gerechnet — melde
    also **regelmäßig**, nicht nur bei Änderungen.

    Es gibt vier Plätze. Derselbe Name landet immer auf demselben; sind alle
    vier mit anderen Namen belegt, kommt ein Fehler statt einer Überschreibung.

    Args:
        params (SignalEingabe): name (str), wert (float, -1 bis 1).

    Returns:
        str: 'ok master.signalN <wert>' oder 'Fehler: <Grund>'.

    Beispiele:
        - „Die Fläche füllt sich" → name='Andrang', wert=0.6
        - „Die Energie kippt" → name='Energie', wert=-0.3
        - Auswerten: `musik_status` zeigt Wert, Trend und Alter je Signal;
          `musik_when('master.signal1', 'unter', -0.2, …)` reagiert darauf.
    """
    namen = [f"master.signal{i}_name" for i in range(1, SIGNALE + 1)]
    try:
        belegt = await _werte(namen)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    platz = next(
        (i for i in range(1, SIGNALE + 1) if belegt.get(namen[i - 1]) == params.name),
        None,
    )
    if platz is None:
        platz = next(
            (
                i
                for i in range(1, SIGNALE + 1)
                if _als_text(belegt.get(namen[i - 1], "-")) is None
            ),
            None,
        )
        if platz is None:
            vergeben = ", ".join(
                f"{i}: {belegt.get(namen[i - 1])}" for i in range(1, SIGNALE + 1)
            )
            return (
                f"Fehler: alle {SIGNALE} Plätze sind belegt ({vergeben}). "
                "Einen freimachen mit musik_set auf signalN_name = '-'."
            )
        # Erst der Name, dann der Wert: Ein Wert auf einem namenlosen Platz
        # sagt niemandem, wovon er handelt.
        antworten = await sprich(
            f"set master.signal{platz}_name {params.name}",
            f"set master.signal{platz} {_zahl_als_text(params.wert)}",
        )
    else:
        antworten = await sprich(
            f"set master.signal{platz} {_zahl_als_text(params.wert)}"
        )

    for antwort in antworten:
        if fehler := _fehlerzeile(antwort):
            return f"Fehler: {fehler}"
    return "\n".join(antworten[-1])


@mcp.tool(
    name="musik_when",
    annotations={"title": "Auf einen Zustand warten", **SCHREIBT},
)
async def musik_when(params: BedingungEingabe) -> str:
    """Merkt einen Befehl für den Moment vor, in dem ein Wert eine Schwelle reißt.

    Das Gegenstück zu `musik_schedule`: Der eine wartet auf Takte, dieser auf
    einen Zustand. „Wenn Deck A noch 32 Beats hat, leg den nächsten auf" ist die
    Frage, die beim Auflegen wirklich gestellt wird.

    **Warum das nicht der Agent selbst macht.** Er müsste `deck1.beats_left` in
    kurzen Abständen abfragen und die Schwelle selbst heraussuchen — je Abfrage
    ein Werkzeugaufruf, also eine Modellantwort weit entfernt. Hier sagt er es
    einmal und ist wieder frei.

    Der Aufruf kommt sofort zurück. Trifft die Bedingung schon jetzt zu, läuft
    der Befehl unmittelbar: `when` heißt „sobald es so weit ist", nicht „beim
    nächsten Überschreiten".

    Args:
        params (BedingungEingabe):
            - control (str): der Wert, etwa 'deck1.beats_left'. Muss eine Zahl
              sein — ein Schalter ließe sich mit keiner Schwelle vergleichen.
            - richtung ('unter'|'ueber'), schwelle (float)
            - **entweder** aktion (str) + argument (str|None)
            - **oder** control_setzen (str) + wert (str)

    Returns:
        str: 'ok plan <nr> …' mit der Nummer für `musik_cancel`, sonst
        'Fehler: <Grund>'.

    Beispiele:
        - „Wenn Deck A fast durch ist, leg den nächsten auf" →
          control='deck1.beats_left', richtung='unter', schwelle=32,
          aktion='master.queue_next'
        - „Bei 16 Beats Rest Deck B starten" → control='deck1.beats_left',
          richtung='unter', schwelle=16, control_setzen='deck2.play', wert='1'
        - Nicht dafür: nach einer festen Zahl Beats → `musik_schedule`.
    """
    zeichen = "<" if params.richtung is Richtung.UNTER else ">"
    befehl = (
        f"when {params.control} {zeichen} "
        f"{_zahl_als_text(params.schwelle)} {params.befehl()}"
    )

    try:
        zeilen = await eine_antwort(befehl)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"

    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}. `musik_list_controls` zeigt, was es gibt."
    return "\n".join(zeilen)


# --------------------------------------------------------------------------
# Was als Nächstes kommt
# --------------------------------------------------------------------------
#
# Nur drei Werkzeuge, obwohl es sieben Aktionen sind: `musik_do` erreicht den
# Rest ohne eine Zeile hier — `master.queue_drop`, `queue_bump`, `queue_note`,
# `queue_clear` nehmen genau ein Argument und stehen mit ihrer Erklärung schon
# in der erzeugten Beschreibung. Ein eigenes Werkzeug bekommt nur, wo sonst die
# Zusammensetzung oder das Auseinandernehmen beim Agenten läge.


@mcp.tool(
    name="musik_queue",
    annotations={"title": "Was als Nächstes kommt", **NUR_LESEN},
)
async def musik_queue(params: ListeEingabe) -> str:
    """Zeigt die Liste der vorgemerkten Tracks, in ihrer Reihenfolge.

    **Die Liste ist gemeinsam.** Wer auswählt, schreibt hier hinein; wer
    auflegt, nimmt hier heraus. Zwei Agenten, die je ihre eigene Liste im Kopf
    führen, legen irgendwann beide auf dasselbe Deck.

    Jeder Eintrag trägt eine Notiz — warum er dort steht. Das ist der
    Unterschied zu einer Playlist, und für den Nächsten, der liest, der ganze
    Punkt.

    Args:
        params (ListeEingabe): limit (1–200), response_format.

    Returns:
        str: Markdown-Liste oder JSON:
        {"count": int, "entries": [{"nr": int, "path": str, "note": str|null}]}
        `nr` spricht einen Eintrag an — für `musik_do('master.queue_drop', …)`,
        `queue_bump` und `queue_note`.

    Beispiele:
        - „Was kommt als Nächstes?"
        - „Warum steht der da?" → das Feld `note`.
        - Nicht dafür: den nächsten auflegen → `musik_queue_next`.
    """
    try:
        zeilen = await eine_antwort("do master.queue")
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"
    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}"

    eintraege = _liste_aus_zeilen(zeilen)[: params.limit]
    if params.response_format is Format.JSON:
        return json.dumps(
            {"count": len(eintraege), "entries": eintraege}, indent=2, ensure_ascii=False
        )

    if not eintraege:
        return "Die Liste ist leer."
    aus = [f"# {len(eintraege)} vorgemerkt", ""]
    for e in eintraege:
        notiz = f" — {e['note']}" if e["note"] else ""
        aus.append(f"{e['nr']}. `{e['path']}`{notiz}")
    return "\n".join(aus)


@mcp.tool(
    name="musik_queue_add",
    annotations={"title": "Einen Track vormerken", **SCHREIBT},
)
async def musik_queue_add(params: VormerkenEingabe) -> str:
    """Merkt einen Track für später vor, mit dem Grund dazu.

    **Derselbe Pfad wird nicht zweimal angenommen** — die Antwort nennt dann
    die Nummer, unter der er schon steht. Das ist der häufigste Zusammenstoß,
    wenn zwei unabhängig voneinander auswählen: Beide suchen, was zu 128 BPM in
    8A passt, und finden denselben Track.

    Args:
        params (VormerkenEingabe):
            - pfad (str): wie ihn `musik_search` liefert
            - notiz (str): warum
            - als_naechstes (bool): an den Anfang statt hinten anhängen

    Returns:
        str: 'queue <nr> angehaengt <pfad>' oder 'Fehler: <Grund>'.

    Beispiele:
        - „Merk den für nachher vor" → pfad=…, notiz='ruhiger, nach dem Peak'
        - „Der soll als Nächstes" → als_naechstes=true
    """
    try:
        zeilen = await eine_antwort(f"do master.queue_add {params.pfad}")
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"
    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}"

    nummer = next(
        (
            teile[1]
            for zeile in zeilen
            if zeile.startswith("queue ") and len((teile := zeile.split(" ", 2))) > 1
        ),
        None,
    )
    if nummer is None:
        return "\n".join(zeilen)

    # Notiz und Vorziehen sind eigene Befehle auf der Leitung. Der Agent soll
    # sie nicht einzeln senden müssen — der Grund gehört zum Vormerken.
    nachtrag = []
    if params.notiz:
        nachtrag.append(f"do master.queue_note {nummer} {params.notiz}")
    if params.als_naechstes:
        nachtrag.append(f"do master.queue_bump {nummer}")
    if nachtrag:
        for antwort in await sprich(*nachtrag):
            if fehler := _fehlerzeile(antwort):
                return f"queue {nummer} angelegt, aber: {fehler}"

    return "\n".join(zeilen)


@mcp.tool(
    name="musik_queue_next",
    annotations={
        "title": "Den nächsten auflegen",
        "readOnlyHint": False,
        # Tauscht den Track eines Decks und nimmt den Eintrag aus der Liste.
        "destructiveHint": True,
        "idempotentHint": False,
        "openWorldHint": False,
    },
)
async def musik_queue_next(params: AuflegenEingabe) -> str:
    """Nimmt den vordersten Eintrag aus der Liste und legt ihn auf.

    Ohne Deckangabe auf eines, das gerade nicht läuft. **Laufen alle, kommt ein
    Fehler statt einer Vermutung** — ein Track über einen laufenden gelegt reißt
    den Mix ab, und das ist keine Entscheidung, die eine Vorgabe treffen darf.

    Scheitert das Laden, bleibt der Eintrag in der Liste stehen.

    Args:
        params (AuflegenEingabe): deck ('deck1'|'deck2'|None).

    Returns:
        str: Die Antwortzeilen — Annahme des Ladeauftrags (nicht Erledigung;
        der Fortschritt steht in 'deckN.load_status'), welcher Eintrag
        abgenommen wurde, und seine Notiz. Sonst 'Fehler: <Grund>'.

    Beispiele:
        - „Leg den nächsten auf"
        - „Leg den nächsten auf Deck 2" → deck='deck2'
    """
    befehl = "do master.queue_next"
    if params.deck:
        befehl += f" {params.deck.value}"

    try:
        zeilen = await eine_antwort(befehl)
    except NichtErreichbar as fehler:
        return f"Fehler: {fehler}"
    if fehler := _fehlerzeile(zeilen):
        return f"Fehler: {fehler}"
    return "\n".join(zeilen)


if __name__ == "__main__":
    mcp.run()
