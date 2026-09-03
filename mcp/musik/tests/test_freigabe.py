#!/usr/bin/env python3
"""Die zwölf Wächter aus `docs/FREIGABE.md`.

Ohne sie wäre die Spezifikation eine Absichtserklärung. Sie brauchen keine
laufende Anwendung — geprüft wird das Gatter, nicht die Anlage.

    python -m pytest mcp/test_freigabe.py
"""

from __future__ import annotations

import datetime as _dt
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from musik import freigabe  # noqa: E402

SCHREIBEND = ("mischen", "spielen", "zeit", "datei", "dramaturgie")


@pytest.fixture(autouse=True)
def ohne_umgebung(monkeypatch):
    """Jeder Test fängt ohne Freigabe an."""
    monkeypatch.delenv("MUSIK_FREIGABE_DATEI", raising=False)


def schreibe(tmp_path, monkeypatch, inhalt: str) -> Path:
    pfad = tmp_path / "musik.freigabe"
    pfad.write_text(inhalt, encoding="utf-8")
    monkeypatch.setenv("MUSIK_FREIGABE_DATEI", str(pfad))
    return pfad


def spaeter(stunden: float) -> str:
    wann = _dt.datetime.now(_dt.timezone.utc) + _dt.timedelta(hours=stunden)
    return wann.isoformat()


# ── Fail-closed ──────────────────────────────────────────────────────────


def test_1_ohne_umgebungsvariable_wird_alles_schreibende_abgewiesen():
    for klasse in SCHREIBEND:
        erlaubt, grund = freigabe.pruefen(klasse, "set channel1.fader 0.8")
        assert not erlaubt, f"{klasse} lief ohne Freigabe"
        assert "MUSIK_FREIGABE_DATEI" in grund


def test_2_fehlende_datei_wird_abgewiesen(tmp_path, monkeypatch):
    monkeypatch.setenv("MUSIK_FREIGABE_DATEI", str(tmp_path / "gibtsnicht"))
    erlaubt, grund = freigabe.pruefen("mischen", "set channel1.fader 0.8")
    assert not erlaubt
    assert "nicht lesbar" in grund


def test_3_abgelaufen_wird_abgewiesen_und_sagt_es(tmp_path, monkeypatch):
    schreibe(tmp_path, monkeypatch, f"klassen mischen\nbis {spaeter(-1)}\n")
    erlaubt, grund = freigabe.pruefen("mischen", "set channel1.fader 0.8")
    assert not erlaubt
    assert "abgelaufen" in grund


def test_4_ohne_zeitzone_wird_nicht_als_ortszeit_geraten(tmp_path, monkeypatch):
    """Der Prozess läuft womöglich woanders als der Mensch, der die Datei
    geschrieben hat. Zwei Stunden mehr Gültigkeit als gedacht ist genau das,
    was nicht passieren soll."""
    schreibe(tmp_path, monkeypatch, "klassen mischen\nbis 2099-01-01T00:00:00\n")
    erlaubt, grund = freigabe.pruefen("mischen", "set channel1.fader 0.8")
    assert not erlaubt
    assert "Zeitzone" in grund


def test_5_zu_weit_voraus_wird_abgewiesen(tmp_path, monkeypatch):
    schreibe(tmp_path, monkeypatch, f"klassen mischen\nbis {spaeter(13)}\n")
    erlaubt, grund = freigabe.pruefen("mischen", "set channel1.fader 0.8")
    assert not erlaubt
    assert "voraus" in grund


def test_6_ein_tippfehler_verwirft_die_ganze_datei(tmp_path, monkeypatch):
    """`mischn` statt `mischen` — dann gilt auch `zeit` nicht. Sonst liefen
    drei von vier Klassen still weiter und niemand merkte den Fehler."""
    schreibe(tmp_path, monkeypatch, f"klassen mischn zeit\nbis {spaeter(2)}\n")
    for klasse in ("mischen", "zeit"):
        erlaubt, _ = freigabe.pruefen(klasse, "irgendwas")
        assert not erlaubt, f"{klasse} lief trotz kaputter Datei"


# ── Was durchgeht ────────────────────────────────────────────────────────


def test_7_eine_gueltige_freigabe_gilt_genau_fuer_ihre_klassen(tmp_path, monkeypatch):
    schreibe(tmp_path, monkeypatch, f"klassen mischen\nbis {spaeter(2)}\nvon felix\n")

    erlaubt, grund = freigabe.pruefen("mischen", "set channel1.fader 0.8")
    assert erlaubt, grund
    assert "felix" in grund

    erlaubt, grund = freigabe.pruefen("datei", "do deck1.load /musik/x.wav")
    assert not erlaubt
    assert "nicht für datei" in grund


def test_8_lesen_ist_immer_frei(tmp_path, monkeypatch):
    """In allen sechs Fehlerfällen oben — Auskunft ändert nichts."""
    erlaubt, _ = freigabe.pruefen("lesen", "get deck1.play")
    assert erlaubt

    schreibe(tmp_path, monkeypatch, "klassen quatsch\nbis kaputt\n")
    erlaubt, _ = freigabe.pruefen("lesen", "get deck1.play")
    assert erlaubt, "lesen wurde von einer kaputten Datei mitgerissen"


def test_9_erzeugen_laeuft_nie_ueber_die_datei(tmp_path, monkeypatch):
    """**Der wichtigste Test.** Ohne ihn kommt die Klasse später aus
    Bequemlichkeit doch in die Datei — und dann gibt ein Agent unbemerkt Geld
    aus."""
    schreibe(tmp_path, monkeypatch, f"klassen erzeugen\nbis {spaeter(1)}\n")
    # Schon das Schreiben der Datei ist ungültig …
    erlaubt, _ = freigabe.pruefen("mischen", "irgendwas")
    assert not erlaubt, "`erzeugen` in der Datei machte sie nicht ungültig"

    # … und selbst mit einer sonst gültigen Datei bleibt erzeugen zu.
    schreibe(tmp_path, monkeypatch, f"klassen mischen zeit\nbis {spaeter(1)}\n")
    erlaubt, grund = freigabe.pruefen("erzeugen", "erzeuge 128 BPM Am")
    assert not erlaubt
    assert "nie" in grund


# ── Widerruf ─────────────────────────────────────────────────────────────


def test_10_datei_loeschen_wirkt_sofort_ohne_neustart(tmp_path, monkeypatch):
    """Der Grund, warum die Freigabe in einer Datei steht und nicht in der
    Umgebung: Wer mitten im Set widerrufen will, kann nicht neu starten."""
    pfad = schreibe(tmp_path, monkeypatch, f"klassen mischen\nbis {spaeter(2)}\n")
    assert freigabe.pruefen("mischen", "set channel1.fader 0.8")[0]

    pfad.unlink()
    erlaubt, grund = freigabe.pruefen("mischen", "set channel1.fader 0.8")
    assert not erlaubt, "der Widerruf wirkte nicht"
    assert "nicht lesbar" in grund


# ── Mitschrift ───────────────────────────────────────────────────────────


def test_11_eine_freigabe_ergibt_eine_notiz(tmp_path, monkeypatch):
    schreibe(tmp_path, monkeypatch, f"klassen mischen zeit\nbis {spaeter(2)}\nvon felix\n")
    f = freigabe.lesen(freigabe.datei_pfad())
    notiz = f.notiz()
    assert notiz.startswith("note freigabe ")
    assert "mischen" in notiz and "zeit" in notiz and "felix" in notiz


def test_12_eine_andere_freigabe_ergibt_eine_andere_notiz(tmp_path, monkeypatch):
    schreibe(tmp_path, monkeypatch, f"klassen mischen\nbis {spaeter(2)}\nvon felix\n")
    erste = freigabe.lesen(freigabe.datei_pfad()).notiz()
    schreibe(tmp_path, monkeypatch, f"klassen mischen\nbis {spaeter(3)}\nvon felix\n")
    zweite = freigabe.lesen(freigabe.datei_pfad()).notiz()
    assert erste != zweite


# ── Die Ablehnung selbst ─────────────────────────────────────────────────


def test_die_ablehnung_nennt_den_befehl_und_verraet_sonst_nichts():
    text = freigabe.verweigert("mischen", "set channel1.fader 0.8", "keine gültige Freigabe")
    assert "set channel1.fader 0.8" in text
    assert "mischen" in text
    assert "MUSIK_FREIGABE_DATEI" in text
    # Kein Wort über den Zustand der Anlage.
    for verraeterisch in ("deck1", "läuft", "Position", "BPM"):
        assert verraeterisch not in text


# ── Die Zuordnung der Controls ───────────────────────────────────────────


def _controls() -> list[str]:
    pfad = Path(__file__).resolve().parents[1] / "controls.txt"
    return [
        z.strip()
        for z in pfad.read_text(encoding="utf-8").splitlines()
        if z.strip() and not z.startswith("#")
    ]


def test_jedes_control_der_anlage_hat_eine_klasse():
    """Sonst wird es im Betrieb abgewiesen — sicher, aber überraschend."""
    ohne = sorted(n for n in _controls() if freigabe.klasse_fuer_control(n) is None)
    assert not ohne, "ohne Klasse: " + " ".join(ohne)


def test_ein_unbekanntes_control_wird_abgewiesen_und_sagt_warum():
    """Fail-closed: lieber abweisen als in die bequemste Klasse rutschen."""
    erlaubt, grund = freigabe.pruefen_control("deck1.gibtsnicht", "set deck1.gibtsnicht 1")
    assert not erlaubt
    assert "unbekanntes Control" in grund


def test_die_familien_werden_richtig_einsortiert():
    """cue1..8, stemN_level, signalN — nummerierte Namen, eine Regel."""
    assert freigabe.klasse_fuer_control("deck1.cue3") == "spielen"
    assert freigabe.klasse_fuer_control("deck1.stem2_level") == "mischen"
    assert freigabe.klasse_fuer_control("deck1.stem2_name") == "lesen"
    assert freigabe.klasse_fuer_control("master.signal1") == "dramaturgie"
    assert freigabe.klasse_fuer_control("master.signal1_trend") == "lesen"


def test_ein_pfad_macht_einen_aufruf_zur_dateisache():
    """Alles, was einen Pfad entgegennimmt, ist `datei` — nicht zeitkritisch,
    also auch nicht in der Set-Freigabe."""
    for name in ("deck1.load", "master.record", "master.queue_add"):
        assert freigabe.klasse_fuer_control(name) == "datei", name


def test_lesende_controls_sind_frei_auch_ohne_freigabe():
    for name in ("deck1.bpm", "deck1.section", "master.arc_gap", "master.transitions"):
        erlaubt, _ = freigabe.pruefen_control(name, f"get {name}")
        assert erlaubt, name
