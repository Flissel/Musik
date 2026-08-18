# Fahrplan: alles, was noch aussteht

`docs/DJ.md` plant den Weg zu einem AI-DJ, der nicht herzlos klingt. Dieses
Dokument ist weiter gefasst: Es nimmt **alles** auf, was offen ist — die
ungebauten Schichten, die ungeprüften Behauptungen und die Dinge, die an
Hardware, Musik oder Ohren hängen und deshalb nicht von hier aus zu erledigen
sind.

Wie dort ist die Reihenfolge das Argument, nicht die Liste.

## Wo das Projekt wirklich steht

Neun von zehn Phasen sind gebaut, und darüber liegen inzwischen drei Schichten
Messwerkzeug: der **Kritiker**, der den Mitschnitt liest; die **Mitschrift**, die
danebenlegt, was gemeint war; und die **Gliederung**, die weiß, wo im Stück ein
Deck steht. Jede davon hat beim Bauen sofort einen Fehler gefunden, den niemand
vermutet hatte.

Zwei Dinge stimmen daran trotzdem nicht.

**Das Messwerkzeug ist selbst ungemessen.** Jede Schwelle in Tempo-, Tonart- und
Strukturerkennung ist an Material geeicht, das ich selbst gebaut habe. In diesem
Projekt ist genau das schon **viermal** die Vorstufe eines Fehlers gewesen: Die
Güteschwelle beim Tempo war an Klick-Tracks geeicht, die zwischen den Klicks
still sind; bei der Tonart wies eine synthetisch geeichte Schwelle vier von fünf
echten Aufnahmen ab; und bei der Gliederung war der Prüfstein „gleichförmiges
Material" ein reiner Sinus, dessen spektraler Fluss um 45 % driftet. Jedes Mal
sah es vorher gut aus.

**Das Team hat nie existiert.** Der Zweck dieses Projekts ist ein Team von
Agenten, das die Anlage gemeinsam bedient. Gelaufen ist bisher **ein** Bediener
in mehreren Rollen. Die Schutzmechanismen dagegen, dass einer dem anderen den
Regler wegzieht, sind gebaut und einzeln geprüft — erlebt hat sie keiner.

## Zwei Sätze, die die Reihenfolge tragen

**Was misst, muss selbst gemessen sein.** Ein Kritiker, dem niemand widerspricht,
ist ein Orakel. Solange die Eichung nur aus eigener Hand kommt, ist jede Zahl in
diesem Repo eine Behauptung über eigenes Material und keine über Musik.

**Was der Zweck ist, muss zuerst laufen.** Alles Weitere — Bogen, Gesten, Raum —
ist für ein Team gedacht, das es noch nicht gibt. Eine Schicht für einen
Bediener zu bauen und später auf mehrere umzustellen, heißt sie zweimal zu
bauen.

Daraus folgt eine Reihenfolge, die zwei Dinge vorzieht, die nach außen wie
Umwege aussehen.

## P1 — Der Prüfstand ✅

*Gebaut. Macht die Eichung überprüfbar, ohne neue Musik.*

Ein Programm, das einen Ordner Tracks und eine **Angabe von Hand** nebeneinander
legt und meldet, wo die Analyse zustimmt und wo nicht:

```text
musik-pruefstand wahrheit.txt
```

Die Wahrheitsdatei liegt neben der Musik, eine Zeile je Track:

```text
# was ich höre
Nachtschicht.mp3  bpm 124  tonart Am  intro 0:00  aufbau 0:32  drop 1:04  outro 5:12
Alpenglühen.wav   bpm 126
```

Abschnitte stehen mit ihrem **Anfang**, nicht als Bereich: Wer zuhört, notiert
„hier fängt das Outro an" und nicht „das Outro geht von … bis …". Jede Angabe
ist einzeln freiwillig — weglassen ist besser als raten.

Der Bericht sagt je Track und in der Summe, wie weit Tempo, Tonart und
Abschnittsgrenzen daneben liegen. Nicht als Note — als Abstand mit Vorzeichen,
wie beim Kritiker.

**Warum das zuerst kommt.** Es ist dasselbe Argument, das den Kritiker vor die
Strukturanalyse gestellt hat, und das hat sich ausgezahlt: Der Kritiker hat
sofort gezeigt, dass meine Schätzung des Blendenbeginns vier Sekunden zu spät
lag — eine Zahl, die vorher niemand hatte.

**Und es macht deinen Beitrag billig.** Ohne Prüfstand heißt „prüf das mal an
echter Musik nach": fünf Tracks laden, Berichte lesen, Zahlen von Hand
vergleichen, mir erzählen, was rauskam. Mit Prüfstand heißt es: Tracks
hinlegen, eine Textzeile je Track schreiben, ein Befehl. Der Aufwand
verschiebt sich vom Nachrechnen zum Hinhören, und hinhören kannst nur du.

**Was er nicht kann.** Eine Angabe von Hand ist eine Meinung. Zwei Leute setzen
den Anfang eines Drops eine Phrase auseinander, und beide haben recht. Der
Prüfstand meldet deshalb Abweichungen, keine Fehler — und ab welcher Abweichung
etwas kaputt ist, entscheidet weiterhin ein Mensch.

Er hat am ersten Tag einen Kunden gehabt: Das gebaute Material aus der
Strukturanalyse trägt seine Wahrheit bereits (128 BPM, A-Dur, Grenzen bei
Sekunde 15, 30, 45, 60, 75). Der erste Lauf dagegen:

```text
  Tempo    127.99 gegen  128.00 gehört   -0.01   ✓
  Tonart  A    (11B) gegen A (11B) gehört   ✓
  Gliederung  6 erkannt gegen 6 gehört
        0.0s  intro   Grenze +0.5s (+1.0 Beats)   Name ✓
       15.0s  aufbau  Grenze +0.5s (+1.0 Beats)   Name ✓
       …
    ── Davon systematisch: +1.0 Beats auf allen Grenzen ──
```

Und dabei kam gleich etwas heraus, das die reine Tabelle verschluckt hätte:
**Jede** Grenze liegt um dasselbe daneben, keine streut. Ein konstanter Abstand
ist eine Tatsache und keine sechs — er sagt etwas über den Nullpunkt (der Anker
des Beatgrids sitzt auf dem ersten erkannten Schlag, die Wahrheitsdatei zählt ab
Dateianfang) und nichts über die Segmentierung. Der Prüfstand trennt beides
seither: gemeinsamer Versatz oben, Rest darunter. Streuen die Abweichungen
stärker, als sie gemeinsam verschoben sind, wird nichts als Versatz erklärt —
sonst würde aus „die Segmentierung wackelt" ein beruhigendes „nur der
Nullpunkt".

Eine Vorlage zum Ausfüllen liegt in
[`wahrheit-vorlage.txt`](wahrheit-vorlage.txt).

## P2 — Das Team ✅ (Stufe 1)

*Gebaut: zwei Verbindungen an einem Pult. Zwei unabhängige Modelle stehen aus.*

Zwei oder mehr Bediener gleichzeitig an einem Pult, jeder mit eigener
Verbindung, und ein Aufbau, der die Zusammenstöße **absichtlich** herbeiführt:

- Zwei greifen denselben Fader — die Rampe muss aufgeben, und zwar die richtige.
- Einer streicht den Plan des anderen. Wer merkt es, und woran?
- Zwei merken auf derselben Phrasengrenze etwas vor. Beide feuern; kommt eine
  Reihenfolge heraus, die klingt?
- Zwei nehmen denselben Track aus der Warteschlange.
- Einer lädt auf ein Deck, das der andere gerade startet.

Die Mechanismen dafür sind gebaut: Eine Rampe gibt auf, sobald der Wert nicht
mehr dem entspricht, was sie zuletzt geschrieben hat; die Warteschlange nimmt
denselben Pfad nicht zweimal; der Plan ist das gemeinsame Blatt. **Geprüft ist
jeder Mechanismus für sich, nie ihr Zusammenspiel.**

Zwei Ausbaustufen, und die Reihenfolge ist wichtig:

1. **Zwei Verbindungen, ein Modell.** Ein Aufbau, der zwei Sitzungen fährt und
   die Fälle oben durchspielt. Reproduzierbar, im Test, ohne API-Kosten. Das
   findet die Verklemmungen.
2. **Zwei unabhängige Modelle.** Erst danach, und mit dem Prüfstand daneben.
   Das findet, was Menschen und Modelle unterschiedlich verstehen — und das
   findet man nicht durch Nachdenken.

**Warum vor den musikalischen Schichten.** Bogen (P4) und Raum (P5) sind
Entscheidungen darüber, *was als Nächstes kommt*. Wer die für einen Bediener
baut und danach auf ein Team umstellt, baut sie zweimal. Und ein Fehler im
Zusammenspiel fällt in einer Schicht darüber nur schwerer auf, nicht später weg.

### Was dabei herauskam

**Der Fund stand vor dem ersten Test.** Die Frage „was sieht ein zweiter
Bediener?" genügte: `takt_starten` gab den Rückgabewert des Taktgebers nicht
weiter, sondern verwarf ihn. Damit ging *jede* Meldung des Plans verloren —
fertig, abgebrochen und vor allem **abgelöst**. Ein Agent, dem ein anderer den
Fader wegnahm, erfuhr es nie und plante weiter auf einer Blende, die seit
zwanzig Sekunden tot war. Für einen einzelnen Bediener fiel das nie auf: Er war
derjenige, der den Regler angefasst hat.

Jetzt liegen die Meldungen in einem Ring mit laufender Nummer, und
`sub master.events` holt sie ab. Kein einzelner Wert, weil der Taktgeber alle
5 ms läuft und der Server alle 50 ms vergleicht — neun von zehn Zeilen wären
weg, gerade wenn viel gleichzeitig geschieht. Wer zu langsam liest, bekommt
`warnung N Ereignisse verloren`; über MCP, wo ein Abo nicht hält, gibt es
`master.events` und `master.event_count` zum Fragen.

Neun Zusammenstöße stehen als Test, dazu einer über den echten Socket mit
laufendem Taktgeber und zwei Verbindungen — dort geht es nicht um die Regeln,
sondern darum, ob sich Mutex, Taktgeber und Abo-Thread gegenseitig aushungern.
Sie tun es nicht.

Was die vorhandenen Mechanismen angeht, hielten sie: Die richtige Rampe gibt
auf, derselbe Track wird nicht zweimal abgenommen, und zwei Aufträge auf
derselben Phrasengrenze feuern beide.

**Stufe 2 steht aus:** zwei unabhängige Modelle. Die findet, was Menschen und
Modelle unterschiedlich verstehen, und das findet man nicht durch Nachdenken.

## P3 — Geste und Repertoire (S4) ✅

*Gebaut. Klein, sofort hörbar, seit S1 messbar.*

Zwei Erweiterungen:

**Formen für Rampen** ✅. `ramp … weich` verteilt dieselbe Strecke als S-Kurve,
`spaet` hält den ausgehenden Track präsent, `frueh` macht den Wechsel zum
Ereignis. Deck und Form stehen hinten in beliebiger Reihenfolge — erkannt wird,
*was* sie sind. Jede Form fängt bei 0 an, kommt bei 1 an und läuft nie zurück;
das sind Wächter, keine Behauptungen.

Offen bleibt die Crossfader-Kurve, die es schon gibt und die im ersten Lauf
ungenutzt auf weich stand — sie ist kein Code, sondern eine Frage der Benutzung.

**Ein Repertoire statt eines Handgriffs** ✅. `do master.uebergang
<blende|bassswap|schnitt|filter> [beats]`. Jeder Griff ist eine Handvoll
gewöhnlicher Protokollzeilen, die durch denselben Weg laufen wie alles andere —
also im Plan stehen, in der Mitschrift, bei den Ereignissen, und mit `cancel`
zurückgehen. Die Antwort nennt jede Zeile: Es gibt nichts, was ein Agent nicht
auch selbst hätte tippen können.

**Die Anlage wählt dabei nicht aus.** Welcher Griff passt, hängt am Kontext —
Outro, Intro-Länge, Energieunterschied, und vor allem daran, was vorher schon
dreimal gefahren wurde. Seit S2 stehen die Zahlen dafür im Steuerraum; die
Entscheidung gehört dem, der sie begründen kann. Nähme die Anlage sie ab,
verlöre das Set genau den Teil, um den es diesem Projekt geht.

Offen bleibt der Loop-Ausstieg — er braucht die Schleifen im Zusammenspiel mit
dem Zeitplan und ist ein eigener Schritt.

Ein DJ, der immer dasselbe macht, ist der langweiligste im Raum — und genau das
war mein erster Lauf. Der Kritiker misst inzwischen Länge und Form jeder Blende,
also lässt sich diesmal belegen, dass sich etwas geändert hat.

## P4 — Der Bogen (S3) ✅

*Gebaut. Vom Übergang zum Set.*

Eine Ziel-Energiekurve über die Setdauer, gegen die jede Auswahl begründet wird.
Die Warteschlange trägt die Notiz dann nicht mehr als Freitext, sondern als
Bezug auf den Bogen: „Plateau halten", „Bruch vorbereiten", „danach wieder
hoch".

Hier gehört auch **Zurückhaltung** einprogrammiert. Ein System, das jeden
Übergang gleich lang und gleich weich fährt, klingt nach Automat — auch wenn
jeder einzelne davon sauber ist.

### Was daraus geworden ist

`master.arc` trägt die Kurve als Text (`0 0.3, 20 0.7, 45 0.95, 60 0.5`),
`do master.arc_start` setzt die Uhr, und dann steht im Steuerraum:
`arc_target`, `arc_actual`, **`arc_gap`** und `arc_trend`. Damit ist
`when master.arc_gap > 0.3 do master.queue_next` sagbar.

**Die Ist-Energie kommt aus der Art des Abschnitts, nicht aus dem Pegel.** Der
Pegel der Gliederung ist auf den lautesten Abschnitt *desselben* Tracks bezogen
— der Drop eines leisen Stücks steht dort genauso bei 0,99 wie der eines lauten.
Über Tracks hinweg ist das nicht vergleichbar, und ein Bogen, der solche Zahlen
addiert, rechnet mit Äpfeln. Stattdessen eine grobe Leiter über die sechs
Abschnittsarten. Sechs Stufen sind hier ehrlicher als eine Nachkommastelle, die
niemand einlösen kann.

Ohne `arc_start` gibt es keinen Ort auf dem Bogen, und dann wird auch keiner
behauptet: leer statt null. Eine Kurve ohne Uhr ist ein Bild, kein Maßstab.

Ein Fund nebenbei, vom Wächter über alle schreibbaren Controls: `master.arc`
nimmt Text an, aber nicht *jeden* Text. Ein Feld, das Text annimmt, ist nicht
dasselbe wie eines, das beliebigen Text annimmt — und ein unlesbarer Bogen wird
abgewiesen statt halb übernommen. Ein Set gegen eine Kurve zu fahren, die
niemand gemeint hat, wäre schlimmer als eines ohne Kurve.

**Die Zurückhaltung steht ✅.** Das Repertoire aus P3 macht Abwechslung möglich,
erzwingt sie aber nicht, und ein System, das viermal hintereinander dieselbe
Blende wählt, klingt weiterhin nach Automat. `master.repeats` sagt, wie viele
der letzten Übergänge gleich waren, `master.transitions` zeigt die letzten acht.
Damit ist `when master.repeats > 2 do master.uebergang filter` sagbar.

Die Zählregel war der ganze Aufwand: **gezählt wird der Seitenwechsel des
Crossfaders, nicht sein Wert.** Eine Rampe schreibt bei jedem Takt in denselben
Regler; auf den Wert gezählt stünde nach einer einzigen Blende eine
Wiederholung von 80 da — die Zahl, die vor Eintönigkeit warnen soll, wäre selbst
der Grund, sie zu ignorieren. Beide Wächter dafür wurden geprüft, indem der
Fehler wieder eingebaut wurde: 4 statt 1, und `schnitt` statt `weich/8`.

Was der letzte Schritt einer Blende und ein Schnitt gemeinsam haben, ist der
geschriebene Wert; unterscheiden lassen sie sich nur über den Schreiber. Deshalb
sagt der Zeitplan dem Pult um den einen Schreibvorgang herum, dass gerade eine
Rampe fährt. Und ein angeforderter Griff wird nur *vorgemerkt* — eingetragen
wird er, wenn der Crossfader ankommt, weil ein abgelöster Griff nicht
stattgefunden hat.

Der zweite Fund kam nicht aus einem Test, sondern aus dem laufenden Programm:
Nach `set channel1.fader 1; set master.crossfader -1; set deck1.play 1` stand
dort bereits `transitions schnitt` — niemand hatte übergeblendet, es lief ja
erst ein Deck. Seitdem gilt zusätzlich, dass **zwei Decks laufen müssen**; alle
vier Griffe starten das eingehende Deck, bevor der Fader sich bewegt.

Die Grenzen stehen im Modul: Wer nur mit den Kanalfadern mischt und den
Crossfader stehen lässt, taucht nicht auf; und läuft der ausgehende Track aus,
bevor die Blende ankommt, fehlt sie. Dann sagt die Zahl nichts, statt etwas
Falsches zu sagen.

Für ein Team ist der Bogen zugleich das, worüber man sich einig sein muss: Ohne
ihn verhandeln zwei Agenten über den nächsten Track ohne gemeinsamen Maßstab.

## P5 — Der Raum schließt die Schleife (S6) ✅

*Brauchte P4, sonst gäbe es nichts zu lenken.*

Die vier Signalplätze existierten, aber im ersten Lauf war der Raum Deko: Werte
gingen hinein, und nichts hat darauf reagiert. Das Set war durchprogrammiert.

Jetzt beugt der **Trend** eines Signals das Ziel des Bogens (`master.room`,
`master.room_bend`) und damit `arc_gap` — und `do master.search_next` sortiert
die Sammlung danach, mit einem Grund je Zeile:

```text
weil Andrang fällt (-0.40/min), Ziel 0.70 → 0.50; es läuft 0.90, weniger gesucht (-0.40)
track 126.00 8A /musik/blaue-stunde.wav Blaue Stunde — Energie 0.48 (-0.02 zum Ziel 0.50), harmonisch
```

Drei Entscheidungen tragen das:

**Der Raum verschiebt das Ziel, nicht die Kurve.** `master.arc` bleibt, was
jemand aufgeschrieben hat; `arc_curve` zeigt es unverändert. Am Ende des Abends
lässt sich so vergleichen, was geplant war und was geschah — eine Anlage, die
ihren eigenen Plan überschreibt, hat nichts mehr, woran sie sich messen ließe.

**Gebeugt wird nach dem Trend, nicht nach dem Wert.** Die Höhe eines Signals ist
nichts Vergleichbares: Was „0,6 Andrang" bedeutet, weiß nur, wer den Sender
geschrieben hat. Eine Änderung ist dagegen eine Aussage über denselben Sender.
Das ist derselbe Fehler, der beim Ist-Wert des Bogens schon einmal auffiel —
zweimal fast hineingelaufen, zweimal an derselben Stelle.

**Höchstens 0,25 Beugung.** Ein hängender oder falsch skalierter Sender darf das
Set nicht übernehmen; das wäre schlimmer als ein Set, das den Raum ignoriert.

Die Energie eines Tracks kommt aus seiner Gliederung, nach Länge gewichtet, und
liegt in der Analyse-Datei neben dem Track statt in der Datenbank — sie hängt am
Inhalt, nicht am Eintrag, und ein neu analysierter Track bringt sie mit, ohne
dass jemand die Sammlung nachpflegt. **Ein nicht analysierter Track steht hinten
und sagt das.**

**Und sie misst keine Lautstärke.** Die Abschnittsarten werden je Track gegen
dessen eigene Quantile benannt; der Drop eines leisen Stücks heißt genauso
„Drop". Was herauskommt, ist, wie viel seiner Länge ein Track auf seinem eigenen
Höhepunkt verbringt — für den Aufbau eines Sets die brauchbarere Größe, für die
Frage „welches Stück ist härter?" die falsche. Das steht so im Modul, weil genau
diese Verwechslung in diesem Projekt schon zweimal ein Fehler war.

Offen bleibt die Quelle. Ein Mikrofonpegel, eine Umfrage auf dem Handy, ein
Mensch, der im Chat „wird voller" tippt — das ist eine Entscheidung über den
Aufbau im Raum und keine Softwarefrage. Bis dahin ist die Schleife gebaut und
läuft mit der Hand am Regler.

## P6 — Stems (S5)

*Teuer, räumt aber den lautesten Fehler weg.*

Zwei Stimmen gleichzeitig sind der hörbarste Mixfehler überhaupt, und ohne
Trennung lässt er sich nur vermeiden, indem man gar nicht überlagert. Mit Stems:
Stimme des ausgehenden Tracks wegnehmen, Instrumental stehen lassen, drüber die
neue Stimme.

Steht zuletzt, weil es **Streaming von Platte** braucht — vier Decks mit je vier
Stems passen nicht in den Speicher — und damit einen Eingriff in den
Abspielpfad, also in den einzigen Teil mit Echtzeitauflagen. Der größte
Klanggewinn und das größte Risiko liegen hier beieinander.

## Was an dir hängt

Diese Punkte kann ich nicht abarbeiten. Was ich tun kann, ist sie **billig
machen** — deshalb steht der Prüfstand vorn.

| | Was fehlt | Was ich vorbereite |
| --- | --- | --- |
| **M1** | **Musik mit Angabe, wo Intro, Drop und Outro anfangen.** Der größte Hebel überhaupt: Damit wird die Benennung der Abschnitte aus einer Vermutung ein Befund | steht: `musik-pruefstand` und [`wahrheit-vorlage.txt`](wahrheit-vorlage.txt) |
| **M2** | **Die Suno-Prompts zu den Tracks** — daraus ergibt sich die Tonart, und damit ist prüfbar, ob die Erkennung stimmt | steht: derselbe Prüfstand, Spalte `tonart` |
| **M3** | **Ein Interface mit vier Ausgängen.** Phase 1 ist bis auf einen Schritt abgenommen: ob der Treiber die Pufferkanäle 3/4 auf die Buchsen 3/4 legt. Ohne Vorhören fehlt dem System außerdem das Ohr am Kopfhörer | vierkanaliges Rendern steht (`musik-mix --cue`), gemessen ist es auch |
| **M4** | **Einmal hinhören, ob die Zeitstreckung bei ±8 % taugt.** Gemessen ist sie, gehört nie | eine Datei mit demselben Takt bei 0,92 / 1,00 / 1,08 zum Vergleichen |
| **M5** | **Eine echte `collection.nml`** — der Traktor-Import ist nie gegen eine gelaufen | der Import steht, der Testfall ist synthetisch |
| **M6** | **Material unter 70 BPM.** Dort findet der Detektor nichts, und ob das an der Grenze liegt oder am Verfahren, ist unbekannt | siehe N3: Das kann ich zur Hälfte selbst |

Vier davon sind mit denselben fünf Tracks erledigt, wenn du sie noch einmal
hochlädst — und diesmal schreibe ich sie sofort ins Repo, statt sie im
Container liegen zu lassen. Drei Container-Resets haben sie inzwischen
verschluckt.

## Nachweise, die ich allein führen kann

Kein Feature, sondern das Einlösen offener Behauptungen. Läuft nebenher.

**N1 — Langsames Material selbst bauen.** Unter 70 BPM findet der Detektor
nichts, weil `MIN_BPM` dort liegt. Ob das Verfahren darunter trägt, lässt sich
mit gebautem Material und **zwei unabhängigen Gegenproben** klären — Oktavlage
und Feinsuche über die Hüllkurve, genau wie bei der Prüfung an echter Musik. Was
dabei herauskommt, ist eine Aussage über das Verfahren, nicht über Musik; das
gehört dazugesagt.

**N2 — Die Gliederung an schwierigen Fällen.** Ein Track ohne Outro, einer ohne
Intro, einer mit zwei Breaks, einer mit einem Tempowechsel. Gebaut, mit bekannter
Wahrheit, durch den Prüfstand. Findet, wo die Regeln zu eng sind.

**N3 — Der Kritiker gegen sich selbst.** Denselben Mitschnitt mit und ohne
Mitschrift bewerten und die Abweichung über viele Sets sammeln. Aus dem einen
gemessenen Wert (3,7 s zu spät) wird so eine Verteilung.

## Kleine Schulden

Nichts davon blockiert etwas; sie stehen hier, damit sie nicht vergessen werden.
Pitch Bend; Reverb als fünfter Effekt; FLAC statt WAV für den Mitschnitt; mehr
als zwei Decks (heute fest verdrahtet); Windows (keine Unix-Sockets — dort
bräuchte es einen zweiten Weg für die Steuerung); MIDI-Controller (Phase 10).

## Reihenfolge und Begründung

| | Was | Warum dort | Hängt an |
| --- | --- | --- | --- |
| P1 ✅ | Prüfstand | Macht jede spätere Zahl prüfbar und deinen Beitrag billig | — |
| P2 ◐ | Das Team | Der Zweck des Projekts; alles darüber wäre sonst zweimal gebaut | Stufe 2: zwei Modelle |
| P3 ✅ | Geste und Repertoire | Sofort hörbar, seit S1 messbar | — |
| P4 ✅ | Der Bogen | Braucht S2; für ein Team der gemeinsame Maßstab | P2 |
| P5 ✅ | Der Raum | Braucht den Bogen, sonst gibt es nichts zu lenken | P4, Quelle |
| P6 | Stems | Größter Klanggewinn, größtes Risiko, braucht Streaming | — |
| M1–M6 | Musik, Gerät, Ohren | Nicht von hier aus zu erledigen | dir |
| N1–N3 | Nachweise | Läuft nebenher | P1 |

**P1 und P2 sind zusammen die kleinste Menge, nach der das Projekt das prüfen
kann, wofür es gebaut ist.** Sie kommen zuerst, obwohl P3 schneller klingt — und
P2 hat den Aufwand schon beim Hinsehen gerechtfertigt.

## Was hier bewusst nicht steht

**Eine Note für ein Set.** Der Kritiker misst Handwerk. Ob ein Set gut war,
entscheidet weiterhin jemand, der dabei war — und das soll so bleiben.

**Eine Generierungsschicht.** Suno hat keine öffentliche API (Stand August
2026), und der Adapter dafür steht in `docs/APIS.md`. Er hängt an Zugang, nicht
an Arbeit hier.

**Termine.** Jede Schicht in diesem Projekt hat unterwegs mindestens einen
Entwurf verworfen, weil das Material widersprochen hat — die Strukturanalyse
allein vier. Eine Schätzung, die das nicht einpreist, wäre gelogen; eine, die es
einpreist, wäre keine Schätzung mehr.
