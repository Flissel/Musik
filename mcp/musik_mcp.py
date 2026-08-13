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
from pydantic import BaseModel, ConfigDict, Field, field_validator

mcp = FastMCP("musik_mcp")

# --------------------------------------------------------------------------
# Verbindung
# --------------------------------------------------------------------------

#: Wie lange auf eine Antwort gewartet wird. Reglerbewegungen sind sofort da;
#: nur `search` fasst eine SQLite-Abfrage an, und auch die ist schnell.
ZEITLIMIT = 5.0

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
    limit: int = Field(default=25, description="Höchstzahl der Treffer", ge=1, le=200)
    response_format: Format = Field(default=Format.MARKDOWN, description="Ausgabeform")

    @field_validator("text")
    @classmethod
    def _einzeilig(cls, v: str) -> str:
        return _pruefe_einzeilig(v, "text")


class StatusEingabe(Basis):
    response_format: Format = Field(default=Format.MARKDOWN, description="Ausgabeform")


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
    """Was gerade läuft: beide Decks, die Kanalzüge und die Summe.

    Der erste Griff, bevor man etwas verändert. Eine Momentaufnahme statt eines
    Dutzends einzelner Abfragen.

    Args:
        params (StatusEingabe): response_format — 'markdown' oder 'json'.

    Returns:
        str: Markdown-Übersicht oder JSON mit dieser Form:
        {
          "decks": [{"deck": "deck1", "title": str, "artist": str,
                     "bpm": float|null, "position": float, "duration": float,
                     "playing": bool, "finished": bool, "load_status": str}],
          "channels": [{"channel": "channel1", "fader": float, "cue": bool,
                        "fx": str}],
          "master": {"crossfader": float, "gain": float, "recording": bool,
                     "record_seconds": float, "record_dropped": float}
        }

    Beispiele:
        - „Was liegt auf den Decks?"
        - „Läuft die Aufnahme noch?"
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


async def _werte(controls: list[str]) -> dict[str, str]:
    """Liest mehrere Controls über eine Verbindung."""
    antworten = await sprich(*(f"get {c}" for c in controls))
    aus: dict[str, str] = {}
    for control, zeilen in zip(controls, antworten):
        zeile = zeilen[-1] if zeilen else ""
        aus[control] = zeile.split(" ", 2)[2] if zeile.startswith("value ") else "-"
    return aus


async def _status(form: Format) -> str:
    vorhandene = {c.name for c in await katalog()}
    decks = sorted({n.split(".")[0] for n in vorhandene if n.startswith("deck")})
    kanaele = sorted({n.split(".")[0] for n in vorhandene if n.startswith("channel")})

    deck_felder = [
        "title",
        "artist",
        "bpm",
        "position",
        "duration",
        "play",
        "finished",
        "load_status",
    ]
    kanal_felder = ["fader", "cue", "fx"]
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
    )
    roh = await _werte(gefragt)

    daten: dict[str, Any] = {
        "decks": [
            {
                "deck": d,
                "title": roh.get(f"{d}.title", ""),
                "artist": roh.get(f"{d}.artist", ""),
                "bpm": _als_zahl(roh.get(f"{d}.bpm", "-")),
                "position": _als_zahl(roh.get(f"{d}.position", "-")) or 0.0,
                "duration": _als_zahl(roh.get(f"{d}.duration", "-")) or 0.0,
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
    """Sucht Tracks in der Sammlung — nach Text oder nach passendem Tempo.

    Args:
        params (SucheEingabe):
            - text (str): Freitext über Titel, Künstler, Album
            - mischbar_zu_bpm (float|None): stattdessen nach Tempo suchen (±6 %)
            - limit (int): 1–200, Standard 25
            - response_format: 'markdown' oder 'json'

    Returns:
        str: Markdown-Liste oder JSON:
        {"count": int, "truncated": bool,
         "tracks": [{"bpm": float|null, "path": str, "title": str}]}
        `path` ist es, was `musik_do` mit der Aktion 'deckN.load' braucht.

    Beispiele:
        - „Was habe ich von Alpenglühen?" → text='Alpen'
        - „Was passt zu 128 BPM?" → mischbar_zu_bpm=128
        - Nicht dafür: einen Track auflegen → `musik_do('deck1.load', pfad)`.
    """
    if params.mischbar_zu_bpm is not None:
        befehl = f"do master.search_mixable {params.mischbar_zu_bpm}"
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
        teile = zeile.split(" ", 2)
        if len(teile) < 3:
            continue
        # `track <bpm> <pfad> <titel>` — der Pfad kann Leerzeichen enthalten,
        # der Titel auch. Getrennt wird deshalb hinten am Titel, der immer der
        # Dateiname ohne Endung oder der Tag ist.
        rest = teile[2]
        pfad, titel = _pfad_und_titel(rest)
        treffer.append({"bpm": _als_zahl(teile[1]), "path": pfad, "title": titel})

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
        zeilen_aus.append(f"- **{t['title']}** — {tempo}\n  `{t['path']}`")
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


if __name__ == "__main__":
    mcp.run()
