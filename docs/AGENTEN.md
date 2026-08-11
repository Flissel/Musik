# Multi-Agenten-Team: Musik aus Crowd-Reaktionen

Fernziel, nicht Phase 1. Dieses Dokument hält die Idee fest, solange sie frisch
ist, und markiert die Stellen, an denen sie an harte Grenzen stößt.

## Die Idee

Ein Team von Agenten übernimmt gemeinsam, was ein DJ tut: die Menge lesen und
entscheiden, was als Nächstes kommt. Der Unterschied zum klassischen DJ-Set —
„was als Nächstes kommt" muss nicht existieren. Es kann generiert werden,
zugeschnitten auf das, was der Raum gerade macht.

Die Crowd wird damit zum Ko-Autor: Sie schreibt keine Noten, aber ihre Reaktion
ist Eingabe in die Komposition.

## Vorarbeit, die es schon gibt

Das ist keine neue Idee, und das ist gut — es gibt Erfahrungswerte.

**hpDJ (HP Labs)** ist das bekannteste Forschungssystem, das DJ-Aufgaben
vollständig automatisiert *und* dabei live auf Crowd-Reaktionen reagiert. Es
misst die Reaktion auf den laufenden Track und lenkt damit die Auswahl des
nächsten. Genau unsere Schleife — nur ohne den Generierungsschritt, weil es den
damals nicht gab.

Heutige kommerzielle AI-DJ-Ansätze nutzen Chat-Nachrichten, Umfragen und
teilweise biometrische Signale, um Tracks, Tonfall und Ansagen zu steuern.

## Signalquellen — von einfach nach heikel

| Quelle | Aufwand | Latenz | Problem |
| --- | --- | --- | --- |
| Chat, Umfragen, Reaktionen | Gering | Sekunden | Nur die lauteste Minderheit antwortet |
| Voting per Handy-App | Mittel | Sekunden | Braucht Installation/QR und aktive Teilnahme |
| Raummikrofon (Pegel, Jubel) | Gering | Sofort | Verwechselt Musiklautstärke mit Publikum |
| Kamera, Bewegungsanalyse | Hoch | Sofort | **Datenschutz** |
| Wearables, Biometrie | Hoch | Sofort | **Datenschutz**, Ausstattung nötig |

⚠️ **Kamera und Biometrie sind in der EU kein Detail.** Gesichtserkennung oder
biometrische Auswertung von Gästen fällt unter die DSGVO und braucht eine
tragfähige Rechtsgrundlage — in einem Club praktisch nur mit ausdrücklicher,
freiwilliger Einwilligung. Das ist keine Zeile Code, sondern ein Konzept mit
Beschilderung, Opt-out und Löschfristen. **Empfehlung: mit anonymen,
aggregierten Signalen starten** (Bewegungsenergie im Bild ohne Personenbezug,
Pegel, Votes). Das liefert überraschend viel und vermeidet den ganzen Komplex.

## Agentenrollen (Entwurf)

```
   Signale ──► [Crowd-Sensor] ──► [Energie-Analyst]
                                         │
                                         ▼
                                  [Set-Planer]  ◄── Set-Historie, Uhrzeit
                                    │      │
                       ┌────────────┘      └───────────┐
                       ▼                               ▼
                [Track-Wähler]                    [Generator]
                (Library)                      (Suno/ElevenLabs)
                       │                               │
                       └──────────────┬────────────────┘
                                      ▼
                               [Mix-Engineer]
                                      │
                                      ▼
                                  Audio-Engine
```

| Rolle | Aufgabe |
| --- | --- |
| **Crowd-Sensor** | Rohsignale einsammeln, normalisieren, anonymisieren |
| **Energie-Analyst** | Aus Signalen einen Zustand ableiten: Energie, Trend, Kipppunkte |
| **Set-Planer** | Dramaturgie über die Zeit — nicht jeder Peak ist der richtige Moment für den Peak |
| **Track-Wähler** | Passendes aus der Library, harmonisch und tempomäßig anschlussfähig |
| **Generator** | Beauftragt neue Tracks, wenn nichts Passendes existiert |
| **Mix-Engineer** | Übergang planen und ausführen: Punkt, Länge, EQ-Kurve |
| **Guardrail** | Vetorecht — verhindert Ausreißer, die den Floor leeren |

## Die harte Grenze: Zeit

Das ist der Punkt, an dem die Idee zuerst bricht, deshalb steht er hier und
nicht im Kleingedruckten.

Ein Dancefloor entscheidet in **Sekunden**. Ein Übergang wird ein bis zwei
Minuten im Voraus geplant. Ein generierter Track braucht — je nach Anbieter —
**Zehner von Sekunden bis Minuten** und ist erst danach überhaupt spielbar.

Die Schleife „Crowd reagiert → Agent generiert → Track spielt" schließt sich
also **nicht** in Echtzeit. Was funktioniert:

1. **Vorausschauend generieren.** Der Set-Planer erzeugt permanent Kandidaten
   für wahrscheinliche nächste Zustände, bevor sie eintreten. Die Crowd steuert
   dann die *Auswahl* aus fertigem Material — das geht sofort.
2. **Echtzeit nur für das, was schnell ist.** EQ, Filter, Loop-Länge,
   Stem-Muting, Pattern-Player-Variationen reagieren unmittelbar. Das ist die
   Ebene, auf der sich „die Musik reagiert auf uns" tatsächlich anfühlt.
3. **Generierung als langsame Schicht.** Sie formt die nächste halbe Stunde,
   nicht die nächsten acht Takte.

Wer das umdreht und auf Echtzeit-Generierung wartet, baut ein System, das immer
zu spät kommt.

## Offene Fragen

- Wie misst man „Energie" so, dass es nicht bloß Lautstärke ist?
- Wie verhindert man Rückkopplung — das System spielt, was gefällt, wird
  dadurch immer glatter und verliert jede Kante?
- Wieviel Kontrolle behält der Mensch am Pult? Vollautomat oder Assistent?
- Wo läuft das Ganze? Auf dem Laptop am Pult oder verteilt?

Die zweite Frage ist die interessanteste. Ein reiner Zustimmungs-Optimierer
konvergiert gegen Mittelmaß. Ein guter DJ nimmt bewusst Risiko — das müsste im
Set-Planer explizit modelliert sein, nicht als Nebeneffekt entstehen.

## Voraussetzungen

Bevor das hier überhaupt sinnvoll wird, müssen stehen:

- Audio-Engine mit zwei Decks und programmierbarem Mixer (Phase 1–2)
- Library mit Metadaten, BPM, Tonart (Phase 4)
- Mindestens ein funktionierender Generierungs-Adapter (siehe [APIS.md](APIS.md))

Das Agenten-Team ist die Schicht *über* dem DJ-Tool. Ohne das Tool darunter ist
es nichts.

## Quellen

- [hpDJ: An Automated DJ with Floorshow Feedback – Springer](https://link.springer.com/chapter/10.1007/1-4020-4097-0_12)
- [Real-Time Adaptive AI DJ Sets: How Audience Feedback Drives Next-Gen Streams](https://www.djcara.com/blog/real-time-adaptive-ai-dj-sets-how-audience-feedback-drives-next-gen-streams/)
- [The Algorithm of the Dancefloor: How DJs Use Data to Command the Crowd – Audiopool](https://www.audiopool.io/all-posts/the-algorithm-of-the-dancefloor-how-djs-use-data-to-command-the-crowd)
