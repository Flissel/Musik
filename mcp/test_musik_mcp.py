#!/usr/bin/env python3
"""Funktionsprobe für den MCP-Server.

Spricht den Server im selben Prozess über einen echten MCP-Client an — also
über `tools/list` und `tools/call`, nicht an der Schnittstelle vorbei. Damit
wird geprüft, was ein Agent wirklich sieht.

Braucht eine laufende `musik-app`. Ohne sie meldet sich der Test ab, statt
Grün zu behaupten, wo nichts geprüft wurde:

    cargo run --release -p musik-app -- --socket /tmp/musik.sock
    MUSIK_SOCKET=/tmp/musik.sock python mcp/test_musik_mcp.py
"""

from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from fastmcp import Client  # noqa: E402

import musik_mcp  # noqa: E402

ERWARTETE_WERKZEUGE = {
    "musik_status",
    "musik_list_controls",
    "musik_get",
    "musik_search",
    "musik_set",
    "musik_do",
}


def text_von(ergebnis) -> str:
    return "\n".join(teil.text for teil in ergebnis.content if hasattr(teil, "text"))


async def hauptteil() -> int:
    if not musik_mcp.KATALOG:
        print(
            "übersprungen: musik-app läuft nicht.\n"
            f"  Socket: {musik_mcp.socket_pfad()}\n"
            "  Starten und MUSIK_SOCKET setzen, dann noch einmal.",
            file=sys.stderr,
        )
        return 77

    fehler: list[str] = []

    def pruefe(bedingung: bool, was: str) -> None:
        print(f"{'ok  ' if bedingung else 'FEHL'} {was}")
        if not bedingung:
            fehler.append(was)

    async with Client(musik_mcp.mcp) as client:
        werkzeuge = await client.list_tools()
        namen = {w.name for w in werkzeuge}
        pruefe(namen == ERWARTETE_WERKZEUGE, f"tools/list nennt {len(namen)} Werkzeuge")

        # Der eigentliche Punkt: Die Beschreibung wird aus dem laufenden
        # Programm erzeugt, nicht von Hand gepflegt.
        setzen = next(w for w in werkzeuge if w.name == "musik_set")
        pruefe(
            "channel1.fader" in (setzen.description or ""),
            "musik_set beschreibt die echten Controls",
        )
        pruefe(
            "0..1" in (setzen.description or ""),
            "musik_set nennt die echten Bereiche",
        )
        ausloesen = next(w for w in werkzeuge if w.name == "musik_do")
        pruefe(
            "deck1.sync" in (ausloesen.description or ""),
            "musik_do beschreibt die echten Aktionen",
        )

        status = text_von(
            await client.call_tool("musik_status", {"params": {"response_format": "json"}})
        )
        daten = json.loads(status)
        pruefe(len(daten["decks"]) >= 2, "musik_status zeigt beide Decks")
        pruefe("recording" in daten["master"], "musik_status kennt den Mitschnitt")

        liste = text_von(
            await client.call_tool(
                "musik_list_controls",
                {"params": {"praefix": "deck1.", "response_format": "json", "limit": 5}},
            )
        )
        katalog = json.loads(liste)
        pruefe(katalog["count"] == 5, "musik_list_controls hält sich an das Limit")
        pruefe(katalog["has_more"], "musik_list_controls meldet, dass mehr da ist")

        gesetzt = text_von(
            await client.call_tool(
                "musik_set", {"params": {"control": "channel1.fader", "wert": "0.42"}}
            )
        )
        pruefe("0.42" in gesetzt, f"musik_set bestätigt den Wert ({gesetzt.strip()})")

        gelesen = text_von(
            await client.call_tool("musik_get", {"params": {"control": "channel1.fader"}})
        )
        pruefe(gelesen.startswith("0.42"), f"musik_get liest ihn zurück ({gelesen})")

        # Begrenzung: 9 ist zu viel für einen Fader, also kommt 1 zurück.
        begrenzt = text_von(
            await client.call_tool(
                "musik_set", {"params": {"control": "channel1.fader", "wert": "9"}}
            )
        )
        pruefe(
            begrenzt.strip().endswith("1"),
            f"musik_set meldet den begrenzten Wert ({begrenzt.strip()})",
        )

        quatsch = text_von(
            await client.call_tool("musik_get", {"params": {"control": "deck1.quatsch"}})
        )
        pruefe(
            quatsch.startswith("Fehler:") and "musik_list_controls" in quatsch,
            "ein unbekanntes Control führt weiter, statt nur zu scheitern",
        )

        aktion_als_wert = text_von(
            await client.call_tool(
                "musik_set", {"params": {"control": "deck1.sync", "wert": "1"}}
            )
        )
        pruefe(
            "Aktion" in aktion_als_wert,
            "eine Aktion mit set zu setzen wird erklärt, nicht verschluckt",
        )

        suche = text_von(
            await client.call_tool(
                "musik_search",
                {"params": {"mischbar_zu_bpm": 128, "response_format": "json"}},
            )
        )
        treffer = json.loads(suche)
        pruefe(treffer["count"] > 0, f"musik_search findet {treffer['count']} Tracks")
        pruefe(
            all(t["path"].endswith((".wav", ".mp3", ".flac")) for t in treffer["tracks"]),
            "die Pfade sind vollständig, auch mit Leerzeichen",
        )

        # Zeilenumbrüche dürfen keinen zweiten Befehl einschleusen.
        try:
            await client.call_tool(
                "musik_set",
                {"params": {"control": "channel1.fader", "wert": "0.5\nset master.gain 0"}},
            )
            pruefe(False, "ein eingeschleuster Befehl wird abgewiesen")
        except Exception:
            pruefe(True, "ein eingeschleuster Befehl wird abgewiesen")

    print()
    if fehler:
        print(f"{len(fehler)} Prüfungen fehlgeschlagen:", file=sys.stderr)
        for f in fehler:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("alles grün")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(hauptteil()))
