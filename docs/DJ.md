# Plan: ein AI-DJ, der nicht herzlos klingt

Der erste vollständig automatisch gefahrene Übergang funktionierte technisch
und klang trotzdem seelenlos. Dieses Dokument sagt, warum, und in welcher
Reihenfolge sich das ändern lässt.

Es ist kein Feature-Katalog. Die Reihenfolge ist das Argument.

## Die Diagnose

Drei Dinge, in aufsteigender Schwere.

**Der Übergang war eine Liste, keine Geste.** Acht Bedingungen, jede an ihrer
eigenen Schwelle, jede unabhängig. Ein DJ denkt nicht in acht Zahlen, sondern in
*einer* Bewegung mit innerem Timing.

**Die Anlage weiß nicht, was sie spielt.** Sie kennt Tempo, Tonart und die
Wellenform. Sie kennt nicht Intro, Break, Drop und Outro — also die einzigen
Stellen, an denen ein Übergang überhaupt sitzen darf. „Blende aus, während das
Outro läuft" ist derzeit nicht ausdrückbar, weil das Wort Outro nicht existiert.

**Und sie hört sich nie selbst.** Der Mitschnitt läuft, aber niemand liest ihn.
Damit ist jede Regel eine Behauptung, jede Verbesserung ein Gefühl, und aus
tausend gefahrenen Sets lernt nichts.

## Die These

Zwei Sätze tragen alles Weitere.

**Jede Entscheidung trägt ihren Grund.** Die Notiz an einem Warteschlangen-
Eintrag ist der Keim davon: nicht „Track 7", sondern „weil der Boden seit vier
Minuten dünner wird und der hier dieselbe Stimmlage hat". Ein System, das seine
Gründe mitschreibt, lässt sich kritisieren; eines ohne sie nur abschalten.

**Jedes Set wird hinterher gemessen.** Ohne das bleibt „klingt besser" eine
Meinung. Mit dem Kritiker wird jede Heuristik zu etwas, das man widerlegen kann
— genau die Wendung, die in diesem Projekt schon dreimal einen Fehler ans Licht
gebracht hat, den niemand vermutet hatte.

Alles andere ist Handwerk in einer bestimmten Reihenfolge.

## S0 — Die Phrase als Einheit

*Klein, sofort, verbessert jeden einzelnen Mix.*

Die Anlage kann „in 32 Beats" und „wenn noch 32 Beats übrig sind". Sie kann
nicht **„auf der nächsten Eins"** — und das ist die Einheit, in der ein Übergang
gedacht wird. `when beats_to_phrase < 1` trifft irgendwo im letzten Beat, nicht
auf den Schlag.

Was fehlt: ein Bezugspunkt `phrase` für die vorhandenen Verben.

```text
at phrase do deck2.sync                  # auf der nächsten Phrasengrenze
at phrase+16 ramp master.crossfader 1 32 # eine Phrase später
ramp channel1.eq_low 0 8 ab phrase       # Bewegung beginnt auf der Eins
```

Damit hängt der ganze Übergang an *einem* musikalischen Zeitpunkt statt an acht
willkürlichen Zahlen. Der Rest verschiebt sich relativ dazu — das ist der
Unterschied zwischen Liste und Geste, und er kostet ein Verb.

## S1 — Der Kritiker

*Macht alles danach messbar. Deshalb so früh.*

Ein Programm, das den Mitschnitt liest und danebenlegt, was beabsichtigt war
(der Plan schreibt es ohnehin mit). Es misst, was sich ohne Geschmacksurteil
messen lässt:

| Maß | Was es findet |
| --- | --- |
| Phrasenlage jedes Übergangs | Startet er auf der Eins oder irgendwo |
| Pegelverlauf über die Blende | Das Loch in der Mitte — beim ersten Lauf 30 % |
| Beat-Drift während der Überlagerung | Ob Sync über die volle Blende hält |
| Chroma-Abstand der überlagerten Teile | Harmonischer Zusammenstoß |
| Länge und Form der Blenden | Ob immer dasselbe gefahren wird |

Ausgabe ist keine Note, sondern ein Befund je Übergang mit Zahl und Zeitpunkt.
Damit lässt sich jede spätere Änderung gegen dieselben Sets prüfen, statt gegen
die Erinnerung.

Das Wichtigste daran: Der Kritiker läuft **offline über den Mitschnitt**. Er
braucht keine neue Analyse im Echtzeitpfad und kann so gründlich sein, wie er
will.

## S2 — Struktur

*Die größte Lücke in der Analyse. Alles Musikalische hängt daran.*

Je Track eine Gliederung in Abschnitte mit Zeitbereichen: Intro, Aufbau, Drop,
Break, Outro. Selbst gebaut aus einer Neuheitskurve über das vorhandene
Beatgrid — Segmentgrenzen liegen dort, wo sich der Klangcharakter über eine
Phrasengrenze hinweg ändert. MIT-verträglich, weil eigen.

Was dadurch erst sagbar wird:

- **Der Übergang sitzt im Outro des einen und im Intro des anderen.** Das ist
  die eigentliche Regel; alles davor war Ersatz.
- **Der Einstiegspunkt des Eingehenden** ist sein erster Downbeat nach dem
  Intro-Beginn, nicht Sekunde 0. Hot Cues gibt es längst — es fehlte nur das
  Wissen, wohin damit.
- **Kein Übergang mitten durch einen Drop.** Der häufigste hörbare Fehler.
- Und für die Auswahl: „ein Track mit langem Intro" ist eine Anforderung, die
  ein Agent stellen kann.

## S3 — Der Bogen

*Vom Übergang zum Set.*

Ein einzelner guter Übergang ist Handwerk. Ein gutes Set ist Architektur: Aufbau,
Plateau, Bruch, Wiederaufbau — über eine Stunde, nicht über vier Minuten.

Konkret: eine Ziel-Energiekurve über die Setdauer, gegen die jede Auswahl
begründet wird. Die Warteschlange trägt die Notiz dann nicht mehr nur als
Freitext, sondern als Bezug auf den Bogen: „Plateau halten", „Bruch vorbereiten",
„nach dem Bruch wieder hoch".

Das ist auch die Stelle, an der Zurückhaltung einprogrammiert gehört. Ein System,
das jeden Übergang gleich lang und gleich weich fährt, klingt nach Automat —
auch wenn jeder einzelne davon sauber ist.

## S4 — Geste statt Rampe

*Wenn die Struktur steht, lohnt sich die Form.*

Zwei Erweiterungen, beide klein:

**Formen für Rampen.** Derzeit linear. Ein Fader, der anfängt zu ziehen,
committet und wieder abflacht, klingt anders als eine Gerade — dieselbe Strecke,
andere Wirkung. Dazu die Crossfader-Kurve, die es schon gibt und die im ersten
Lauf ungenutzt auf weich stand.

**Ein Repertoire statt eines Handgriffs.** Lange Blende, Bass-Swap, harter
Schnitt auf die Eins, Filter-Sweep, Loop-Ausstieg. Welcher passt, entscheidet
der Kontext: Genre, Energiewechsel, ob beide Tracks ein brauchbares Intro/Outro
haben. Ein DJ, der immer dasselbe macht, ist der langweiligste im Raum — und
genau das ist mein erster Lauf gewesen.

## S5 — Stems

*Teuer, aber es räumt den lautesten Fehler weg.*

Zwei Stimmen gleichzeitig sind der hörbarste Mixfehler überhaupt, und ohne
Trennung lässt er sich nur vermeiden, indem man gar nicht überlagert. Mit Stems:
Stimme des ausgehenden Tracks wegnehmen, Instrumental stehen lassen, drüber die
neue Stimme. Das ist der Punkt, an dem Übergänge aufhören, nach Übergang zu
klingen.

Steht in der Roadmap als Phase 8 und braucht Streaming von Platte — vier Decks
mit je vier Stems passen nicht mehr in den Speicher.

## S6 — Der Raum schließt die Schleife

*Erst hier, weil vorher nichts da wäre, worauf man reagieren könnte.*

Die Signale von außen existieren, aber im ersten Lauf war der Raum Deko: Ich
habe Werte hineingegeben, und nichts hat darauf reagiert. Das Set war
durchprogrammiert.

Die Schleife schließt sich, wenn der Trend eines Signals die **Auswahl** und den
**Bogen** verändert, nicht nur einen Regler. Und wenn das System dabei sagt,
warum — „Andrang fällt seit drei Minuten, ich nehme den ruhigeren mit dem langen
Intro statt des Peaks".

Dann ist es kein Automixer mehr.

## Reihenfolge und Begründung

| | Was | Warum dort |
| --- | --- | --- |
| S0 | Phrase als Bezugspunkt | Ein Verb, wirkt auf jeden Mix, blockiert nichts |
| S1 | Kritiker über den Mitschnitt | Macht alles Folgende prüfbar statt behauptbar |
| S2 | Strukturanalyse | Voraussetzung für jede musikalische Regel |
| S3 | Set-Bogen | Braucht S2, um Abschnitte zu planen |
| S4 | Gestenformen und Repertoire | Lohnt erst, wenn die Stellen stimmen |
| S5 | Stems | Teuer, braucht Streaming; größter Klanggewinn |
| S6 | Raum steuert Auswahl und Bogen | Braucht S3, sonst gibt es nichts zu lenken |

S0 und S1 sind zusammen die kleinste Menge, nach der sich das Ergebnis
**belegen** lässt statt nur behaupten. Sie kommen zuerst, auch wenn S2 den
größeren Klanggewinn verspricht.

## Was daran nicht Software ist

- **Material.** Drei Sinustöne und ein Kick haben nichts, was man fühlen könnte.
  Ohne Musik mit Struktur ist jeder Übergang eine Rechenaufgabe.
- **Das Interface mit vier Ausgängen.** Ohne Vorhören fehlt dem System das Ohr,
  das ein DJ am Kopfhörer hat — es kann den nächsten Track nicht prüfen, bevor
  er im Raum ist.
- **Geschmack.** Der Kritiker misst, was messbar ist. Ob ein Set *gut* war,
  entscheidet weiterhin jemand, der dabei war. Der Kritiker soll ihm die
  Handwerksfehler abnehmen, nicht das Urteil.
