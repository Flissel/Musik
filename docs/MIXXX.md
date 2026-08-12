# Mixxx als Referenz

[Mixxx](https://github.com/mixxxdj/mixxx) ist die ausgereifteste freie
DJ-Software, die es gibt — seit rund zwanzig Jahren in Entwicklung, C++ mit Qt,
vier Decks, Sampler, Effekte, DVS/Timecode, Broadcasting, Mitschnitt,
Controller-Mappings für über hundert Geräte. Alles, was hier in Phasen 7 bis 10
noch aussteht, hat Mixxx längst.

Die naheliegende Frage lautet deshalb: warum nicht einfach Mixxx?

Dieses Dokument beantwortet beides — was wir übernehmen können und was nicht,
und unter welchen Umständen „einfach Mixxx nehmen" die richtige Antwort wäre.

## Die Lizenz, genau

Zwei Zahlen, die auseinanderlaufen:

| Ort | Lizenz |
| --- | --- |
| `LICENSE` im Repo | GPL **v2** or later |
| Ein gebautes Binary | faktisch GPL **v3** or later |

Der Unterschied kommt von [libKeyFinder](https://github.com/mixxxdj/libkeyfinder)
(Tonarterkennung, GPLv3), das seit Mixxx 2.3 mitgelinkt wird — GPLv3 ist mit
GPLv2-only unverträglich, die „or later"-Klausel hebt das Ganze also auf v3.

Für uns macht das keinen Unterschied: **Copyleft ist Copyleft.** Mixxx fällt in
dieselbe Schublade wie Rubber Band, aubio und Essentia in
[BAUSTEINE.md](BAUSTEINE.md#lizenzen--entschärft-nicht-erledigt) — nutzbar,
solange nichts weitergegeben wird, und ausgeschlossen, sobald der Anschluss an
[VibeMind](VIBEMIND.md) (MIT, zur Veröffentlichung vorgesehen) ernst wird.

## Lesen ist nicht kopieren

Der Punkt, an dem viele zu vorsichtig sind: **Urheberrecht schützt die
Ausdrucksform, nicht die Idee.** Ein Algorithmus, eine Architektur, die
Erkenntnis „der Cue-Bus muss pre-fader und post-EQ abgegriffen werden" — das
sind keine geschützten Werke. Mixxx zu lesen, um zu verstehen *wie* etwas gelöst
ist, und es danach selbst zu schreiben, ist zulässig und üblich.

Was nicht geht: Code übernehmen, Dateien vendorn, Zeile für Zeile übersetzen.
Eine Rust-Portierung einer C++-Funktion ist eine Bearbeitung, keine Neuschöpfung.

**Praktische Regel für dieses Projekt:** zuerst ins
[Wiki](https://github.com/mixxxdj/mixxx/wiki) und ins
[Handbuch](https://manual.mixxx.org/), nicht in den Quelltext. Beide beschreiben
die Architektur ausführlich genug, und wer die Lösung aus der Prosa
rekonstruiert statt aus dem Code, hat das Problem der versehentlichen Nähe gar
nicht erst.

## Was uns wirklich weiterhilft

Nach absteigendem Wert:

### 1. Die Controller-Mappings als Hardware-Dokumentation

`res/controllers/` enthält über hundert Mappings als `.midi.xml`, `.hid.xml`
und `.js` — Pioneer, Denon, Numark, Hercules, Traktor Kontrol, Allen & Heath
und so weiter, teils vom Team zertifiziert, teils aus der Community.

Der Inhalt ist zweierlei, und die Trennung ist wichtig:

- **Welche MIDI-CC-Nummer das Jogwheel eines DDJ-400 sendet**, ist eine
  *Tatsache über die Hardware*. Tatsachen sind nicht urheberrechtlich
  geschützt. Als Nachschlagewerk dafür, wie die Geräte-Landschaft aussieht, ist
  das Verzeichnis Gold wert — genau das, was Phase 10 sonst mühsam durch
  Ausprobieren am Gerät ermitteln müsste.
- **Die Mapping-Dateien selbst** sind Werke unter GPL. Nicht kopieren, nicht
  mitliefern, nicht konvertieren.

Also: als Doku lesen, eigenes Format schreiben.

### 2. Das Control-System als Vorbild für die MCP-Schnittstelle

Mixxx hat eine interne API, über die **alles** läuft — Tastatur, MIDI, HID und
die grafische Oberfläche greifen auf dieselben benannten Controls zu, adressiert
über einen zweiteiligen `ConfigKey` aus Gruppe und Element, etwa
`[Channel1],play`.

Das ist genau die Form, die unsere Anbindung an VibeMind braucht: ein flacher,
benannter Steuerraum, den ein Agent adressieren kann, ohne die Oberfläche zu
kennen. Wir haben das bisher nicht — Transport läuft über Atomics im
`DeckState`, der Mixer über die Kommandoschlange, beides ohne gemeinsame
Namensgebung. Bevor die MCP-Schicht aus [VIBEMIND.md](VIBEMIND.md) gebaut wird,
lohnt sich dieser Zwischenschritt.

Architektonische Idee, frei übernehmbar.

### 3. Zwei Beatgrid-Stufen statt einer

Mixxx bietet die Wahl zwischen zwei Analysatoren, und der Unterschied ist genau
der, den wir noch vor uns haben:

| Verfahren | Ergebnis |
| --- | --- |
| SoundTouch | eine mittlere BPM-Zahl → **konstantes** Grid |
| Queen Mary | einzelne Beat-Positionen → **variables** Grid |

Unser Modell (Anker + konstante BPM, siehe `crates/analysis/src/tempo.rs`) ist
die erste Stufe. Das ist für produzierte elektronische Musik richtig und
billig. Für live eingespieltes Material — Bands, ältere Aufnahmen, alles ohne
Klick — schwankt das Tempo, und ein konstantes Grid läuft über vier Minuten
sichtbar weg.

Damit ist der Aufrüstpfad benannt: eine Beat-Liste statt Anker+BPM, und
`Beatgrid` müsste beides können. Kein akutes Problem, aber ein Grund, die
Schnittstelle nicht auf „eine Zahl" festzunageln.

### 4. Streaming von Platte

Mixxx dekodiert Tracks nicht vollständig in den Speicher, sondern liest mit
einem Vorpuffer nach. Genau die offene Entscheidung aus
[PLAN.md](PLAN.md#offene-entscheidungen), die spätestens bei Stems fällig wird —
vier Decks à vier Stems sprengen den Vollentschlüsselungs-Ansatz.

Das Wiki beschreibt den Aufbau. Lesen lohnt, bevor wir das selbst entwerfen.

### 5. Library-Importe, die wir nicht haben

Wir lesen Traktors `collection.nml`. Mixxx liest zusätzlich Rekordbox und
Serato, beides durch Rückentwicklung erarbeitet. Wenn diese Formate je
gebraucht werden, ist dort dokumentiert, wie sie aussehen — wieder:
Formatwissen ist Tatsache, der Parser ist Werk.

## Was Mixxx *nicht* löst

**Steuerung von außen.** Mixxx spricht OSC nur in eine Richtung: es *sendet*
Zustand (Titel, Position, Lautstärke, Play-Status) an einen OSC-Server.
Das Empfangen von Befehlen ist ein seit Jahren offener Wunsch mit
Community-Patches, aber kein offizieller Bestandteil. Das Control-System selbst
ist prozessintern.

Für einen Agenten, der auflegen soll, ist Mixxx damit keine fertige Basis. Genau
diese Lücke ist das, was dieses Projekt eigentlich beiträgt — nicht der
zwanzigste Mixer, sondern ein DJ-Werkzeug, das von Anfang an eine
Steuerschnittstelle nach außen hat.

**Die Lizenz für den VibeMind-Weg.** Einbetten geht nicht, und ein Fork wäre
GPL. Beides schließt die Veröffentlichung als MIT aus.

## Und wenn wir doch Mixxx nehmen?

Der Vollständigkeit halber, weil es die ehrliche Alternative ist:

> **Solange dies ein persönliches Werkzeug bleibt und nicht weitergegeben wird,
> entstehen aus Mixxx' GPL null Pflichten.** Es ist fertige, ausgereifte
> Software, die alles kann, was hier noch drei Phasen entfernt ist. Wer heute
> Abend auflegen will, nimmt Mixxx.

Die Gründe, trotzdem selbst zu bauen, sind genau zwei:

1. **Der Anschluss an VibeMind** — der braucht MIT und Veröffentlichung.
2. **Die Steuerung durch Agenten** — die es in Mixxx nicht gibt und die sich
   nicht nachrüsten lässt, ohne den Kern anzufassen (und damit zu forken, und
   damit unter GPL zu landen).

Fallen beide Ziele weg, ist der Eigenbau schwer zu rechtfertigen. Bleiben sie,
ist er der einzige Weg. Das ist keine technische, sondern eine Zielfrage — und
sie sollte bewusst beantwortet und nicht durch Weiterbauen umgangen werden.

## Quellen

- [mixxxdj/mixxx auf GitHub](https://github.com/mixxxdj/mixxx) —
  [LICENSE](https://github.com/mixxxdj/mixxx/blob/main/LICENSE) (GPLv2+),
  [`res/controllers/`](https://github.com/mixxxdj/mixxx/tree/main/res/controllers)
- [PR #3476 — Lizenzhebung auf GPLv3 wegen libKeyFinder](https://github.com/mixxxdj/mixxx/pull/3476)
- [mixxxdj/libkeyfinder](https://github.com/mixxxdj/libkeyfinder) — GPLv3
- [Developer Guide: Control](https://github.com/mixxxdj/mixxx/wiki/Developer-Guide-Control) — das ConfigKey-System
- [Developer Guide: Analysers](https://github.com/mixxxdj/mixxx/wiki/Developer-Guide-Analysers) — SoundTouch gegen Queen Mary
- [Handbuch: Beat Detection](https://manual.mixxx.org/2.3/id/chapters/preferences/beat_detection)
- [Wiki: OSC Client](https://github.com/mixxxdj/mixxx/wiki/osc-client) und
  [OSC Backend](https://github.com/mixxxdj/mixxx/wiki/Osc-Backend)
- [Wiki: Contributing Mappings](https://github.com/mixxxdj/mixxx/wiki/Contributing-Mappings)

Stand: August 2026. Nicht selbst nachgebaut — alles aus Dokumentation und
Repository-Metadaten, bewusst nicht aus dem Quelltext.
