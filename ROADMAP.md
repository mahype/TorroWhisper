# Open Whisper — Roadmap

Sammlung geplanter Features und Ideen. Reihenfolge ist nicht final priorisiert.

---

## 1. Chat-Funktion (Voice-Assistant-Modus)

**Status:** Konzept festgehalten, Detail-Plan in `~/.claude/plans/lass-uns-bitte-mal-gleaming-nest.md`

### Idee
Open Whisper bekommt eine zweite Hauptfunktion neben dem Diktat: einen **Chat-Modus** mit eigenem, frei konfigurierbarem Shortcut. Statt den transkribierten Text in die aktive App einzufügen, spricht der User mit einer KI und bekommt eine **gesprochene Audio-Antwort**.

### Flow
```
Chat-Shortcut → Audio-Aufnahme → Whisper-Transkription
            → KI-Provider (Chat-Completion) → TTS → Audio-Ausgabe
```

### Design-Entscheidungen
| Bereich | Wahl |
|---|---|
| KI-Provider | OpenAI (ChatGPT), Anthropic (Claude), Google Gemini, Ollama/LM Studio, **Gemma als Default-Download** (klein/mittel/groß, bis 31B) |
| TTS | macOS System TTS (`AVSpeechSynthesizer`) — plattform-abstrahiert für späteren Support anderer OS |
| Integration | Neuer Mode-Typ: Modes bekommen ein `kind`-Feld (`dictation` \| `chat`); Chat-Shortcut aktiviert den aktiven Chat-Mode |
| Konversation | Multi-Turn mit Timeout (z. B. 5 Min Inaktivität → neue Session) |
| API-Keys | macOS Keychain (`Security.framework`), nicht in Settings-JSON |

### Offene UX-Details
- Trigger: Toggle vs. Push-to-Talk (per Mode konfigurierbar?)
- Streaming-TTS (Satz-für-Satz während LLM noch generiert) vs. blocking
- Interrupt: Shortcut während TTS → stoppt und startet neue Frage
- Optionales Floating-Chat-Fenster mit Transkript-Historie
- Sprache: Whisper-Sprache vs. TTS-Stimme bei mehrsprachiger Antwort

### Scope
**v1:** Chat mit allen Cloud-Providern + lokales Gemma + System-TTS + Multi-Turn
**Später:** Cloud-TTS (OpenAI, ElevenLabs), Tool-Use, paralleles Multi-Session, Windows/Linux-TTS

---

## 2. Dictionary / Wort-Ersetzungen

### Problem
Whisper transkribiert bestimmte Wörter systematisch falsch. Beispiel: **"committe"** wird konsequent als **"komm bitte"** geschrieben. Solche Fehler wiederholen sich identisch und sind heute nur durch manuelle Nachkorrektur lösbar.

### Idee
Ein **benutzerdefiniertes Dictionary**, das nach der Transkription läuft und definierte Strings ersetzt:

```
"komm bitte"  →  "committe"
"git hub"     →  "GitHub"
"react js"    →  "React.js"
...
```

### Anforderungen
- Eintrags-Verwaltung in den Settings (eigene "Dictionary"-Sektion): Liste mit `[Pattern → Replacement]`-Paaren, Add/Edit/Delete
- Optional pro Eintrag: Case-sensitive ja/nein, Whole-Word-Match ja/nein
- Läuft **vor** dem optionalen LLM-Post-Processing (sonst macht das LLM den Fehler ggf. wieder rückgängig)
- Wirkt sowohl im Diktat-Modus als auch im Chat-Modus (auf transkribierte Eingabe, bevor sie an die KI geht)
- Per-Mode aktivierbar oder global?

### Offene Fragen
- Regex-Support oder nur Plain-String?
- Soll Whisper selbst das Dictionary kennen (via `initial_prompt`)? Das könnte die Trefferquote erhöhen, ist aber tokenlimitiert.
- Default-Dictionary mit häufigen Tech-Begriffen vorinstalliert (commit, GitHub, npm, React, …)?

---

## 3. Adaptives Lernen aus Nachkorrekturen

### Idee
Wenn der User den eingefügten Text **direkt nach dem Diktat** im Eingabefeld ändert, soll Open Whisper diese Änderung erkennen, lernen und beim nächsten Mal automatisch anwenden — eine wachsende Form des Dictionary aus Punkt 2, aber **automatisch befüllt**.

### Flow (Konzept)
1. Diktat fügt Text X in App ein
2. App beobachtet das aktive Eingabefeld für ein kurzes Zeitfenster (z. B. 30 s)
3. User editiert → App diff't Original-X gegen Endzustand
4. Wenn klares Pattern erkennbar (z. B. wiederkehrende Substring-Ersetzung), wird Vorschlag generiert: *"Soll 'X' zukünftig automatisch zu 'Y' werden?"*
5. User bestätigt → Eintrag landet im Dictionary

### Technische Hürden
- **Beobachtung des Eingabefelds nach dem Insert** ist auf macOS heikel (Accessibility-API, Permission, viele Apps liefern keinen sauberen Read-Back)
- **Diff-Heuristik**: Wann ist eine Änderung "lernenswert" vs. "Userbezogene Umformulierung"? Vermutlich nur kurze, lokale Substring-Ersetzungen vorschlagen.
- **Privacy**: Der gesehene Text darf nirgends persistiert werden außer im Dictionary nach Bestätigung.

### Mögliche Vereinfachung als v1
- Kein Live-Beobachten, sondern: **manueller Lern-Shortcut** ("Mark last as correction") — User markiert nach manueller Korrektur, Open Whisper holt sich den letzten Insert + den aktuellen Eingabefeld-Inhalt, schlägt Dictionary-Eintrag vor.

---

## 4. Auto-Korrektur Toggle

### Idee
Manchmal ist nach der Whisper-Transkription klar, dass der Text Rechtschreib-/Grammatikfehler enthält (z. B. erkennbar an Wörtern, die im Wörterbuch nicht existieren). Eine optionale **automatische Korrektur** kann das beheben — soll aber **per Schalter in den Einstellungen** an-/abschaltbar sein.

### Anforderungen
- Settings-Toggle (z. B. unter "Recording" oder "Modes"): "Automatische Korrektur (Rechtschreibung/Grammatik)" an/aus
- Per-Mode konfigurierbar (manche Modes wollen rohes Transkript, andere geputzten Text)
- Implementierungs-Optionen:
  - **Lightweight**: System-Spell-Checker von macOS (`NSSpellChecker`) — kostenlos, lokal, schnell, nur Rechtschreibung
  - **Heavyweight**: LLM-basiertes Cleanup über bestehende Post-Processing-Pipeline (Ollama/Gemma/Cloud) — mächtiger, langsamer, teurer
- Vermutlich beide Optionen anbieten: "Aus" / "Spell-Check" / "LLM-Cleanup"

### Reihenfolge der Pipeline (mit allen Features)
```
Whisper-Transkription
  → Dictionary-Replace (Punkt 2)
  → Auto-Korrektur (Punkt 4, falls aktiviert)
  → LLM-Post-Processing (bestehender Mode-Prompt, falls aktiviert)
  → Insert in App (Diktat) ODER an Chat-LLM (Chat-Mode)
```

---

## Allgemeine Querschnitt-Überlegungen

- **Reihenfolge der Verarbeitungsstufen** muss sauber definiert sein und ggf. pro Mode konfigurierbar
- **Performance**: Dictionary-Replace ist O(n·m), bei vielen Einträgen Aho-Corasick statt naivem Loop
- **Plattform-Abstraktion**: TTS, Spell-Check und Eingabefeld-Beobachtung sind alle plattformspezifisch — Trait-basierte Abstraktion in Rust mit macOS-Implementation als erstem Backend
