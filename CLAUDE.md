# CLAUDE.md

Guidance for AI agents (and humans) working in this repository.

## Issues müssen KI-optimiert geschrieben werden

**Jedes Issue, das wir anlegen, MUSS so formuliert sein, dass es anschließend von
einer KI gelesen und eigenständig umgesetzt werden kann.** Issues sind in erster
Linie Anweisungen für eine KI, nicht nur Notizen für Menschen.

Konkret heißt das:

- **Vollständige Anweisungen.** Es darf keine Anweisung verloren gehen. Alles, was
  umgesetzt werden soll, steht explizit im Issue — die KI hat keinen Zugriff auf den
  Chat-Verlauf, in dem das Issue entstanden ist.
- **Gedankengänge festhalten.** Auch die Überlegungen, Begründungen und
  Entscheidungen, die im Chat besprochen und festgelegt wurden, gehören ins Issue.
  Wenn wir uns auf einen Ansatz geeinigt haben, muss das *Warum* mit dokumentiert
  werden, damit die umsetzende KI den Kontext und die Absicht versteht.
- **Selbst-enthalten.** Das Issue muss aus sich heraus verständlich sein: relevante
  Datei-/Pfadangaben, betroffene Crates/Module, erwartetes Verhalten,
  Akzeptanzkriterien und ggf. Edge-Cases explizit benennen.
- **Eindeutig und umsetzbar.** Klare, konkrete Schritte statt vager Wünsche. Wo
  sinnvoll, konkrete Akzeptanzkriterien oder Definition-of-Done angeben.
- **Sprache.** Issues auf Deutsch verfassen (Projektsprache), technische Begriffe
  bleiben exakt.

Faustregel: Eine KI, die *nur* das Issue liest (ohne diesen Chat), muss die Aufgabe
korrekt und vollständig umsetzen können.
