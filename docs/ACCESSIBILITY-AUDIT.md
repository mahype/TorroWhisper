# WCAG 2.1 Accessibility Audit — DonnyWhisper (macOS)

**Datum:** 2026-06-10 · **Basis:** Code-Audit aller SwiftUI/AppKit-UI-Dateien.
**Hinweis:** WCAG 2.1 gilt formal für Web-Inhalte. Für native Software gilt
[WCAG2ICT](https://www.w3.org/TR/wcag2ict/) — dieselben Erfolgskriterien, übertragen auf
Nicht-Web-Software. Dieses Audit prüft gegen Level A + AA.

Statisches Code-Audit — kein Ersatz für manuelle Tests mit VoiceOver,
Accessibility Inspector, „Reduce Motion" und „Increase Contrast".

---

## Was bereits gut ist

- `NSAccessibility.post(.announcementRequested)` für Phasenwechsel (Recording/Transcribing/Post-Processing) in `AppDelegate.announceRuntimeTransition()` und für Mikrofonwechsel-Toast.
- Stop-Button in der Recording-Bubble hat `.accessibilityLabel()` + `.help()`.
- AppUIComponents: dekorative Icons via `.accessibilityHidden(true)`, `.isSelected`-Traits, kombinierte Labels für History-Einträge, Download-Progress mit Label + Value.
- HotkeyRecorderField: Fehler/Warnungen mit „Error:"/„Warning:"-Prefix-Labels + VoiceOver-Announcement bei Fehlern.
- Durchgängige Lokalisierung via `L()`, auch für Accessibility-Strings.
- Native NSMenus (Menüleiste) sind von Haus aus tastatur- und VoiceOver-bedienbar.

---

## Priorität 1 — Level-A/AA-Blocker

### 1. Recording-Bubble für VoiceOver/Tastatur unerreichbar
`AppDelegate.swift` (Panel-Setup, `.nonactivatingPanel`, teils `ignoresMouseEvents`) + `RecordingIndicatorView.swift`
- Das schwebende Panel ist nicht zuverlässig im Accessibility-Tree; Stop-Button per Tastatur nicht erreichbar (Panel wird nie key).
- Kein Tab-Zugang zum Stop-Button → einzige Alternative ist der Hotkey.
- **WCAG:** 2.1.1 (Keyboard, A), 4.1.2 (Name/Role/Value, A)
- **Fix:** Panel als Accessibility-Element exponieren, bei `.recording` fokussierbar machen oder dokumentierten, immer verfügbaren Tastatur-Stopp garantieren (Hotkey existiert — dann in Announcement nennen, s. Punkt 8).

### 2. Hartkodierte Schriftgrößen — Text nicht skalierbar
- `RecordingIndicatorView.swift`: 9–11 pt für Modellname, Modus, Hotkey-Hinweis.
- `MicSwitchToastView.swift:9,13`: `size: 18` / `size: 13`.
- `AppUIComponents.swift:546`: `size: 10` (Step-Nummern).
- **WCAG:** 1.4.4 (Resize Text, AA) — Text muss bis 200 % vergrößerbar sein.
- **Fix:** Semantische Stile (`.caption`, `.caption2`, `.body`) statt fixer Größen; mit „Larger Text" testen.

### 3. Reduce Motion wird ignoriert
`RecordingIndicatorView.swift`: Waveform-Animation (30-Hz-Polling, `.animation(.linear…)`) und blinkender Status-Dot (`statusDot`, Zeile 184–202) laufen bedingungslos.
- Blinkmuster: 16 Slots/s, ~25 % aus → >3 Wechsel/s. 8-px-Punkt liegt unter der 2.3.1-Flächenschwelle (kein Seizure-Blocker), aber für motion-sensitive Nutzer problematisch.
- **WCAG:** 2.3.3 (Animation, AAA — Apple-HIG-Pflicht trotzdem), 2.2.2 (Pause/Stop/Hide, A — blinkende Inhalte)
- **Fix:** `@Environment(\.accessibilityReduceMotion)` auswerten: statischer Dot, eingefrorene/gedimmte Waveform. Blinkfrequenz generell auf ≤1–2 Hz senken.

### 4. Menüleisten-Icon: Zustand nur visuell
`AppDelegate.swift` (`statusImage()`): `megaphone` vs. `megaphone.fill`, `accessibilityDescription` immer „DonnyWhisper".
- VoiceOver-Nutzer hören nie, ob Aufnahme läuft.
- **WCAG:** 1.4.1 (Use of Color/visuell allein, A), 4.1.2 (A)
- **Fix:** Description zustandsabhängig setzen („DonnyWhisper — Aufnahme läuft"), Tooltip ebenso.

### 5. Status nur über Farbe signalisiert
- `SettingsView.swift` (~623–685): Runtime-Status-Circle rot/grün/orange/lila ohne Accessibility-Label am Indikator; Text daneben existiert nur teilweise.
- `OnboardingView.swift` (~228, 364, 420): grüne Checkmark-Icons für „erteilt/geladen" ohne `.accessibilityLabel`.
- `RecordingIndicatorView.swift` (`statusDotColor`): rot/gelb/orange als Phasensignal (Text-Label existiert daneben — prüfen, ob es alle Zustände abdeckt).
- **WCAG:** 1.4.1 (A), 1.1.1 (A)
- **Fix:** Status-Indikatoren `.accessibilityLabel` mit Klartext geben oder als `accessibilityElement(children: .combine)` mit dem Text koppeln.

### 6. Kontrast-Probleme (hartkodierte/getönte Farben)
- `LanguageModelsManagerSheet.swift:252,347,442`: AccentColor-Text auf `accentColor.opacity(0.14)`-Badge — bei hellen Akzentfarben unter 4.5:1.
- `AppUIComponents.swift:401`: Orange Text auf `orange.opacity(0.18)` („Cancelled"-Badge) — fällt voraussichtlich durch.
- `AppUIComponents.swift:115`: Trash-Icon disabled mit `secondary.opacity(0.35)` — sehr schwach.
- `LanguageModelsManagerSheet.swift:184,214`, `SettingsView.swift:627ff`: `.red`-Fehlertext auf Form/Material-Hintergrund — messen.
- **WCAG:** 1.4.3 (Kontrast Text, AA), 1.4.11 (Non-text Contrast, AA)
- **Fix:** Kontraste messen (Accessibility Inspector / Sim Daltonism), Badge-Texte auf `.primary` mit farbigem Hintergrund umstellen oder dunklere Farbtöne verwenden.

### 7. HotkeyRecorderField: Capture-NSView ohne Accessibility
`HotkeyRecorderField.swift` (`HotkeyCaptureView`, NSViewRepresentable):
- Fängt alle keyDown-Events; keine `accessibilityRole`/`Label`/`Hint` am NSView. VoiceOver-Nutzer wissen nicht, dass sie in einem Capture-Modus sind und dass Escape abbricht → faktisch Keyboard-Trap-Risiko.
- Keyboard-Icon (Zeile ~32) ohne Label.
- **WCAG:** 2.1.2 (No Keyboard Trap, A), 4.1.2 (A)
- **Fix:** Accessibility-Attribute am Capture-View („Hotkey-Aufnahmefeld. Tastenkombination drücken. Escape bricht ab, Rücktaste löscht."), Announcement beim Start des Capture-Modus.

### 8. Statusmeldungen unvollständig / Escape nicht kommunizierbar
- `AppDelegate.announceRuntimeTransition()`: Announcements nennen weder Stop-/Cancel-Möglichkeit noch Hotkey; Hotkey-Hinweis in der Bubble nur als Symbole (⌃⇧Space).
- Onboarding: Schrittwechsel im Wizard wird nicht announced (`OnboardingView.swift`).
- Escape als Cancel-Taste ist hartkodiert (Carbon-Handler, keyCode 53) — nicht remappbar.
- **WCAG:** 4.1.3 (Status Messages, AA), 2.1.4 (Character Key Shortcuts, A — Escape ohne Modifier)
- **Fix:** Announcements erweitern („Transkription läuft. Escape bricht ab."), Hotkeys in Announcements ausschreiben, Wizard-Schritte announce­n, Escape-Cancel konfigurierbar machen (Roadmap).

---

## Priorität 2 — AA, geringeres Risiko

9. **MicSwitchToastView**: Icon ohne `.accessibilityHidden(true)` bzw. Label; hartkodiert weiß-auf-schwarz (funktioniert, aber „Increase Contrast"/Farbschema-unabhängig prüfen); Auto-Dismiss 2,8 s — Announcement existiert (gut), reicht damit für 2.2.1, aber Announcement erst nach `orderFrontRegardless()` posten (Race).
10. **SettingsView Slider (VAD-Threshold, ~449–464)**: kein eigenes `accessibilityLabel`/`accessibilityValue` am Slider — Wert (ms) wird VoiceOver nicht klar zugeordnet. (4.1.2)
11. **LanguageModelsManagerSheet:157,164**: „+ Choose file…"-Buttons — „+" wird vorgelesen; Label ohne „+" setzen. (1.1.1)
12. **AppDelegate Submenüs** (Mode/Model/Mic/History): beim Erstellen `setAccessibilityLabel` setzen, nicht erst beim Befüllen. (1.3.1)
13. **RecordingIndicatorView Waveform**: kein `accessibilityLabel` am Waveform-Container („Audiopegel-Anzeige"). (1.1.1)
14. **Onboarding-Wizard**: `navigationTitle` wechselt, Fokus springt nicht zum neuen Inhalt; Fokus-Management pro Schritt ergänzen. (2.4.3)
15. **Icon-Only-Buttons in History** (`AppUIComponents.swift:418,425`): Labels vorhanden, aber Klickfläche klein — `.frame(minWidth: 28, minHeight: 28)`+ erwägen. (2.5.5, AAA — optional)

---

## Empfohlene Reihenfolge

1. Punkte 2 + 3 (Fonts, Reduce Motion) — mechanisch, geringes Risiko, großer Effekt.
2. Punkte 4 + 5 + 13 (Labels/Status) — reine Additionen.
3. Punkt 1 + 7 (Bubble-Erreichbarkeit, Hotkey-Capture) — braucht Design-Entscheidung.
4. Punkt 6 (Kontrast) — erst messen, dann gezielt fixen.
5. Punkt 8 (Announcements/Escape-Remap) — teilweise Roadmap-Thema.

## Manuelle Test-Checkliste (nach Fixes)

- [ ] VoiceOver: kompletter Durchlauf Onboarding → Settings → Diktat → History.
- [ ] Nur Tastatur (Full Keyboard Access an): alle Dialoge, Sheet, Hotkey-Feld, Stopp/Abbruch.
- [ ] System­einstellungen → Bedienungshilfen: „Reduce Motion", „Increase Contrast", „Larger Text" jeweils an.
- [ ] Accessibility Inspector: Kontrast-Audit über Settings, Sheets, Bubble (Light + Dark Mode).
