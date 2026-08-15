#!/usr/bin/env python3
"""Funktionsprobe für den MCP-Server.

Spricht den Server im selben Prozess über einen echten MCP-Client an — also
über `tools/list` und `tools/call`, nicht an der Schnittstelle vorbei. Damit
wird geprüft, was ein Agent wirklich sieht.

Der Test **verändert die laufende Anlage**: Er bewegt Fader, merkt Tracks vor
und legt auf, wenn kein Deck läuft. Nicht auf einer Anlage laufen lassen, an der
gerade jemand arbeitet.

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
    "musik_ramp",
    "musik_schedule",
    "musik_cancel",
    "musik_queue",
    "musik_queue_add",
    "musik_queue_next",
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
    uebersprungen: list[str] = []

    def pruefe(bedingung: bool, was: str) -> None:
        print(f"{'ok  ' if bedingung else 'FEHL'} {was}")
        if not bedingung:
            fehler.append(was)

    def ausgelassen(was: str, warum: str) -> None:
        print(f"--   {was} — {warum}")
        uebersprungen.append(f"{was} ({warum})")

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
        pruefe(
            isinstance(daten.get("plan"), list),
            "musik_status zeigt den gemeinsamen Plan",
        )
        pruefe(
            isinstance(daten.get("queue", {}).get("count"), int),
            "musik_status sagt, wie viel vorgemerkt ist",
        )

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
        pruefe(
            all("key" in t for t in treffer["tracks"]),
            "jeder Treffer trägt eine Tonart-Spalte, auch wenn sie leer ist",
        )

        # Eine Tonart, die keine ist, muss auffallen und darf nicht wie ein
        # leeres Ergebnis aussehen.
        unsinn = text_von(
            await client.call_tool(
                "musik_search", {"params": {"harmonisch_zu": "H-Dur"}}
            )
        )
        pruefe(
            unsinn.startswith("Fehler:"),
            "eine unlesbare Tonart wird gemeldet statt leer zu antworten",
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

        # ------------------------------------------------------------------
        # Zeit: Blenden und vorgemerkte Befehle
        # ------------------------------------------------------------------

        async def plan_jetzt() -> list[dict]:
            roh = text_von(
                await client.call_tool(
                    "musik_status", {"params": {"response_format": "json"}}
                )
            )
            return json.loads(roh)["plan"]

        # Ein Schalter ist keine Bewegung — das muss auffallen, bevor
        # irgendetwas vorgemerkt wird.
        kein_regler = text_von(
            await client.call_tool(
                "musik_ramp",
                {"params": {"control": "deck1.play", "ziel": 1, "ueber_beats": 8}},
            )
        )
        pruefe(
            kein_regler.startswith("Fehler:"),
            "eine Blende auf einem Schalter wird abgewiesen",
        )

        # Ein halber Befehl ist schlimmer als gar keiner: Er sähe aus, als wäre
        # etwas vorgemerkt.
        for was, eingabe in [
            ("weder Aktion noch Control", {"beats": 8}),
            (
                "beides zugleich",
                {
                    "beats": 8,
                    "aktion": "deck2.sync",
                    "control": "channel1.fader",
                    "wert": "1",
                },
            ),
            ("ein Control ohne Wert", {"beats": 8, "control": "channel1.fader"}),
        ]:
            try:
                await client.call_tool("musik_schedule", {"params": eingabe})
                pruefe(False, f"musik_schedule weist {was} ab")
            except Exception:
                pruefe(True, f"musik_schedule weist {was} ab")

        try:
            await client.call_tool("musik_cancel", {"params": {}})
            pruefe(False, "musik_cancel ohne Ziel streicht nicht einfach alles")
        except Exception:
            pruefe(True, "musik_cancel ohne Ziel streicht nicht einfach alles")

        vorher = await plan_jetzt()
        hat_grid = any(d["bpm"] for d in daten["decks"])
        if not hat_grid:
            ausgelassen(
                "Blende und Vormerkung",
                "kein Deck hat ein Beatgrid, also gibt es keine Beats",
            )
        else:
            eq_vorher = text_von(
                await client.call_tool(
                    "musik_get", {"params": {"control": "channel1.eq_low"}}
                )
            ).strip()

            blende = text_von(
                await client.call_tool(
                    "musik_ramp",
                    {
                        "params": {
                            "control": "channel1.eq_low",
                            "ziel": 0,
                            "ueber_beats": 64,
                        }
                    },
                )
            ).strip()
            pruefe(blende.startswith("ok plan "), f"musik_ramp blendet ({blende})")

            # Dasselbe, aber erst in 32 Beats — die Zusammensetzung, die ein
            # Agent wirklich braucht.
            spaeter = text_von(
                await client.call_tool(
                    "musik_ramp",
                    {
                        "params": {
                            "control": "channel2.eq_low",
                            "ziel": 0,
                            "ueber_beats": 16,
                            "in_beats": 32,
                        }
                    },
                )
            ).strip()
            pruefe(
                spaeter.startswith("ok plan "),
                f"musik_ramp kann auch warten, bevor sie losfährt ({spaeter})",
            )

            vorgemerkt = text_von(
                await client.call_tool(
                    "musik_schedule",
                    {"params": {"beats": 64, "aktion": "deck2.sync"}},
                )
            ).strip()
            pruefe(
                vorgemerkt.startswith("ok plan "),
                f"musik_schedule legt einen Befehl auf einen Beat ({vorgemerkt})",
            )

            alte_ids = {a["id"] for a in vorher}
            neu = [a for a in await plan_jetzt() if a["id"] not in alte_ids]
            pruefe(len(neu) == 3, f"alle drei Aufträge stehen im Plan ({len(neu)})")
            arten = sorted(a["art"] for a in neu)
            pruefe(
                arten == ["in", "in", "ramp"],
                f"der Plan sagt, welcher Art die Aufträge sind ({arten})",
            )

            weg = text_von(
                await client.call_tool(
                    "musik_cancel", {"params": {"plan_id": neu[0]["id"]}}
                )
            ).strip()
            pruefe(weg == "ok 1 gestrichen", f"musik_cancel nimmt einen zurück ({weg})")

            if vorher:
                # Da steht fremde Arbeit im Plan — die wird nicht mitgestrichen.
                ausgelassen(
                    "musik_cancel mit alle=true",
                    "es standen schon fremde Aufträge im Plan",
                )
                for a in neu[1:]:
                    await client.call_tool(
                        "musik_cancel", {"params": {"plan_id": a["id"]}}
                    )
            else:
                alles = text_von(
                    await client.call_tool("musik_cancel", {"params": {"alle": True}})
                ).strip()
                pruefe(
                    alles == "ok 2 gestrichen",
                    f"musik_cancel räumt mit alle=true auf ({alles})",
                )
                pruefe(not await plan_jetzt(), "danach ist der Plan leer")

            # Die Blende hat den EQ ein Stück bewegt; er kommt zurück.
            if eq_vorher not in ("-", ""):
                await client.call_tool(
                    "musik_set",
                    {"params": {"control": "channel1.eq_low", "wert": eq_vorher}},
                )

        # ------------------------------------------------------------------
        # Was als Nächstes kommt
        # ------------------------------------------------------------------

        async def liste_jetzt() -> list[dict]:
            roh = text_von(
                await client.call_tool(
                    "musik_queue", {"params": {"response_format": "json"}}
                )
            )
            return json.loads(roh)["entries"]

        vorher_liste = await liste_jetzt()
        pfade = [t["path"] for t in treffer["tracks"][:2]]

        angehaengt = text_von(
            await client.call_tool(
                "musik_queue_add",
                {"params": {"pfad": pfade[0], "notiz": "warm anfangen"}},
            )
        ).strip()
        pruefe(
            angehaengt.startswith("queue "),
            f"musik_queue_add merkt einen Track vor ({angehaengt})",
        )

        # Zwei, die unabhängig auswählen, finden denselben Track. Ihn zweimal
        # aufzunehmen hieße, ihn zweimal zu spielen.
        doppelt = text_von(
            await client.call_tool(
                "musik_queue_add", {"params": {"pfad": pfade[0], "notiz": "auch gut"}}
            )
        ).strip()
        pruefe(
            doppelt.startswith("Fehler:") and "Nummer" in doppelt,
            f"derselbe Pfad wird nicht zweimal angenommen ({doppelt})",
        )

        eintraege = await liste_jetzt()
        meiner = next((e for e in eintraege if e["path"] == pfade[0]), None)
        pruefe(meiner is not None, "der Eintrag steht in der Liste")
        pruefe(
            meiner is not None and meiner["note"] == "warm anfangen",
            f"die Notiz kommt mit ({meiner and meiner['note']})",
        )

        if len(pfade) > 1:
            await client.call_tool(
                "musik_queue_add",
                {
                    "params": {
                        "pfad": pfade[1],
                        "notiz": "doch der zuerst",
                        "als_naechstes": True,
                    }
                },
            )
            zuerst = (await liste_jetzt())[0]
            pruefe(
                zuerst["path"] == pfade[1],
                f"als_naechstes zieht nach vorn ({zuerst['path']})",
            )
        else:
            ausgelassen("als_naechstes", "die Sammlung hat nur einen Track")

        # Auflegen verändert ein Deck. Nur, wenn dort ohnehin nichts läuft.
        if any(d["playing"] for d in daten["decks"]):
            ausgelassen("musik_queue_next", "es läuft gerade etwas")
        else:
            vorn = (await liste_jetzt())[0]
            aufgelegt = text_von(
                await client.call_tool("musik_queue_next", {"params": {}})
            )
            pruefe(
                f"queue {vorn['nr']} abgenommen" in aufgelegt,
                f"musik_queue_next nimmt den vordersten ({aufgelegt.splitlines()})",
            )
            pruefe(
                vorn["note"] is None or f"notiz {vorn['note']}" in aufgelegt,
                "die Notiz kommt beim Auflegen mit",
            )
            pruefe(
                not any(e["nr"] == vorn["nr"] for e in await liste_jetzt()),
                "und ist danach aus der Liste heraus",
            )

        # Aufräumen: nur die eigenen Einträge, nie die der anderen.
        alte = {e["nr"] for e in vorher_liste}
        for e in await liste_jetzt():
            if e["nr"] not in alte:
                await client.call_tool(
                    "musik_do",
                    {"params": {"aktion": "master.queue_drop", "argument": str(e["nr"])}},
                )

    print()
    if uebersprungen:
        print(f"{len(uebersprungen)} Prüfungen ausgelassen:")
        for u in uebersprungen:
            print(f"  - {u}")
    if fehler:
        print(f"{len(fehler)} Prüfungen fehlgeschlagen:", file=sys.stderr)
        for f in fehler:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("alles grün")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(hauptteil()))
