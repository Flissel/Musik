# Externe APIs und Datenquellen

Stand: August 2026. Recherchestand, keine eigenen Tests — Preise und
Lizenzbedingungen vor jeder Integration selbst prüfen, die ändern sich schnell.

## Kurzfassung

**Suno hat keine öffentliche API.** Das ist der Blocker für die
Generierungs-Features. Es gibt seit Juli 2026 ein Partner-Programm mit
Bewerbungsformular, aber keine offenen Endpunkte, keine Doku, kein Preismodell,
kein Termin.

Konsequenz für die Planung: Wir bauen den Generierungs-Teil **hinter einer
Adapter-Schnittstelle** und starten mit einem Anbieter, an den wir heute
herankommen. Suno wird später ein weiterer Adapter, kein Umbau.

## Musikgenerierung

### Suno — aktuell nicht verfügbar

| | |
| --- | --- |
| Status | Keine self-serve öffentliche API |
| Partner-Programm | Seit Juli 2026, kuratierte Auswahl, Bewerbung per Formular |
| Doku/Preise | Nicht veröffentlicht |
| Lizenzlage | Ungeklärt, Verfahren zu Trainingsdaten laufen |

Suno-CPO Jack Brody hat das Programm im Juli 2026 angekündigt und sucht Apps,
„die Erfahrungen ermöglichen, die generative Musik zum ersten Mal möglich
macht". Ein Multi-Agenten-DJ, der auf Crowd-Reaktionen komponiert, ist genau
dieses Profil — **die Bewerbung ist einen Versuch wert.**

⚠️ **Nicht empfohlen:** Es kursieren reverse-engineerte „Suno APIs" von
Drittanbietern. Die sind inoffiziell, können jederzeit brechen und bewegen sich
verstoßen aller Wahrscheinlichkeit nach gegen Sunos Nutzungsbedingungen. Dass
das Projekt nicht kommerziell ist, ändert daran nichts — das realistische Risiko
ist ein gesperrter Account, nicht eine Lizenzklage.

### Alternativen, die heute nutzbar sind

| Anbieter | Stärke | Lizenzlage | Anmerkung |
| --- | --- | --- | --- |
| **ElevenLabs Music** | Vocals, Songs mit Text; klare Konditionen | Auf lizenzierten Daten trainiert, breit kommerziell freigegeben | Zusatzlizenz nötig für Werbung, Film, TV, Games, Enterprise |
| **Stable Audio** (Stability) | Instrumentale Texturen, transparente Preise | Stability-Lizenz | Drittquellen nennen ~0,20 $/Track |
| **Udio** | Von Musikern für Qualität gelobt | Ungeklärt, wie Suno | Gleiches Risikoprofil |
| **Mubert**, **Beatoven** | Hintergrundmusik, Royalty-free | Sauber strukturiert | Weniger „Track", mehr Bett |
| **Loudly** | Volle kommerzielle Rechte am API-Output | Klar | |
| **Google Lyria** | Vocals, lyric-getriebene Songs | Google-Lizenz | |

**Empfehlung als Startadapter: ElevenLabs Music.** Es gibt einen dokumentierten
`/music/compose`-Endpunkt, die Lizenzlage ist die klarste im Feld, und der
Funktionsumfang deckt das ab, was wir zum Testen der Pipeline brauchen.

Bekannte Parameter von `/music/compose` (laut Doku-Zusammenfassung):

- Entweder ein Text-Prompt **oder** ein detaillierter Kompositionsplan — nicht beides
- Songlänge in Millisekunden, 3 000–600 000 (nur zusammen mit Prompt)
- Modellauswahl
- Seed für reproduzierbare Ergebnisse
- Optionales Flag für rein instrumentale Ausgabe

Der Seed-Parameter ist für uns interessanter, als er klingt: Er macht Varianten
eines Tracks reproduzierbar — brauchbar für Übergänge zwischen zwei Fassungen
desselben Materials.

### Was die Adapter-Schnittstelle abdecken muss

Aus dem Vergleich oben ergeben sich die gemeinsamen Nenner:

```
generiere(prompt | plan, laenge, instrumental?, seed?) -> Job
status(Job) -> pending | fertig | fehler
hole(Job) -> Audio + Metadaten + Prompt-Historie
```

Asynchron, weil alle Anbieter Zeit brauchen. Siehe
[ARCHITEKTUR.md](ARCHITEKTUR.md) — der Audio-Pfad darf davon nie blockiert
werden.

## Samples und Audiomaterial

### Freesound

APIv2 ist die praktikabelste Quelle für Samples mit sauberer Rechtelage.

- Textsuche über die gesamte Bibliothek
- Endpunkt pro Sound: Metadaten, Tags, Content-Analyse-Features, Preview-URLs, Download
- Lizenzen: **CC0, CC BY, CC BY-NC**
- API frei für nicht-kommerzielle Anwendungen; kommerzielle Nutzung ist separat lizenzierbar
- Seit Juli 2026 gibt es eine „Generative AI"-Präferenz pro Upload

**CC BY-NC ist nutzbar**, weil das Projekt nicht kommerziell ist — das ist der
größte praktische Gewinn dieser Entscheidung, denn NC-Material macht einen
erheblichen Teil von Freesound aus.

⚠️ **Die Attributionspflicht bleibt.** CC BY verlangt Namensnennung auch
nicht-kommerziell. Das heißt konkret: Die Library braucht von Anfang an ein Feld
für Lizenz und Urheber pro Track. Nachträglich lässt sich das nicht
rekonstruieren, wenn erst einmal tausend Samples ohne Herkunft im Ordner liegen.

### Weitere Quellen

- **Free Music Archive** — ganze Tracks unter CC
- **ccMixter** — Remix-orientiert, Stems und Acapellas
- **Splice** u. ä. — kommerzielle Sample-Abos, API-Zugang meist nicht offen

## Konkrete nächste Schritte

- [x] Entschieden: **nicht kommerziell.** Damit sind CC-BY-NC-Samples nutzbar
      und die ungeklärte Lizenzlage bei Suno/Udio ist kein Ausschlusskriterium
      mehr — nur noch der fehlende API-Zugang.
- [ ] Bei Suno für das Partner-Programm bewerben (Intake-Formular, Juli 2026 veröffentlicht)
- [ ] ElevenLabs-API-Key besorgen, `/music/compose` einmal manuell gegen curl testen
- [ ] Freesound-API-Key besorgen (die API ist für nicht-kommerzielle Nutzung kostenlos)
- [ ] Lizenz- und Urheberfeld im Library-Schema vorsehen, bevor Samples
      importiert werden

Was die Entscheidung **nicht** löst: Suno hat weiterhin keine öffentliche API.
Der Blocker war nie die Lizenz, sondern der Zugang.

## Quellen

- [Suno Is Opening an API Partner Program – Digital Music News](https://www.digitalmusicnews.com/2026/07/03/suno-is-opening-an-api-partner-program/)
- [The Suno API Reality: A Developer's Guide – AI/ML API Blog](https://aimlapi.com/blog/the-suno-api-reality)
- [Is There a Public Suno API for Developers in 2026 – MusicGPT](https://musicgpt.com/blog/suno-api)
- [Compose music – ElevenLabs Documentation](https://elevenlabs.io/docs/api-reference/music/compose)
- [Eleven Music, now available in the API – ElevenLabs](https://elevenlabs.io/blog/eleven-music-now-available-in-the-api)
- [AI Music Generation API Comparison: The Developer's Guide 2026](https://aimusicapi.ai/en/blog/ai-music-generation-api-comparison)
- [AI Music Generation 2026: Suno, Udio, ElevenLabs Compared – Digital Applied](https://www.digitalapplied.com/blog/ai-music-generation-platforms-suno-udio-elevenlabs-2026)
- [APIv2 Overview – Freesound API documentation](https://freesound.org/docs/api/overview.html)
- [Freesound FAQ](https://freesound.org/help/faq/)
