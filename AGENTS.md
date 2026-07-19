# AGENTS.md

Regeln für KI-Agenten (und Menschen) in diesem Repository. Ergänzt
[`CLAUDE.md`](CLAUDE.md) — dort stehen die Regeln für das Schreiben von Issues,
hier die für Design und Marke sowie für das Arbeiten mit Testinstanzen.

---

## Testinstanzen der App starten

Beim Entwickeln wird die App oft über `./scripts/dev.sh` als **Testinstanz**
(SPM-Binary aus `.build/…/debug/`) gestartet, parallel zur beim Nutzer
**installierten** `/Applications/TorroWhisper.app`. Das ist heikel, weil beide
dieselbe Settings-Datei, denselben globalen Hotkey **und denselben
Autostart-Eintrag** teilen. Deshalb gelten folgende Regeln verbindlich:

**1. Vor dem Start einer Testinstanz immer ALLE anderen Instanzen beenden.**
Das betrifft sowohl frühere Dev-Instanzen als auch die installierte App:

```bash
pkill -f "\.build/.*/debug/TorroWhisper"                 # alte Dev-Instanzen
pkill -f "/Applications/TorroWhisper.app/Contents/MacOS"  # installierte App
```

Zwei laufende Instanzen bedeuten zwei Menüleisten-Symbole und Konkurrenz um
Hotkey und Settings — nie gleichzeitig laufen lassen.

**2. Verhindern, dass die Testinstanz nach einem Neustart automatisch von
macOS gestartet wird.** Die App registriert Autostart über einen LaunchAgent
in `~/Library/LaunchAgents/TorroWhisper.plist` (Rust-Crate `auto_launch`,
`crates/torrowhisper-bridge/src/autostart.rs`). Der Eintrag wird mit
`env::current_exe()` gebaut. Ist in den (geteilten) Settings „Bei Anmeldung
starten" aktiv, schreibt **jede gestartete Instanz beim Start die plist auf
ihren eigenen Pfad um** — die Testinstanz kapert damit den Autostart und zeigt
auf die `.build/debug`-Binary, die nach dem nächsten `swift build`-Clean evtl.
gar nicht mehr existiert.

Nach dem Start der Testinstanz deshalb **die plist prüfen und, falls sie auf
`.build/…` zeigt, zurück auf die installierte App schreiben:**

```bash
PLIST=~/Library/LaunchAgents/TorroWhisper.plist
grep -q "\.build/" "$PLIST" && sed -i '' \
  "s#<string>[^<]*\.build/[^<]*</string>#<string>/Applications/TorroWhisper.app/Contents/MacOS/TorroWhisper</string>#" \
  "$PLIST"
```

(Solange die Testinstanz die Settings nicht neu speichert, bleibt der Eintrag
korrekt — bei einer Settings-Änderung ggf. erneut prüfen.)

**3. Am Ende immer den Nutzer fragen, bevor aufgeräumt wird.** Wenn die Arbeit
mit der Testinstanz erledigt ist, **nicht eigenmächtig** beenden, sondern
fragen: *„Soll ich die Testinstanz beenden und die lokale/installierte App
wieder starten?"* Bei Ja: Dev-Instanz beenden und die installierte App starten
— deren Start registriert den LaunchAgent ohnehin wieder auf den korrekten
`/Applications`-Pfad.

```bash
pkill -f "\.build/.*/debug/TorroWhisper"
open -a /Applications/TorroWhisper.app
```

---

## Design & Marke — Torro

Verbindliche Quelle für alles, was mit der Marke Torro zu tun hat, ist das
Repository **[`mahype/torro-design`](https://github.com/mahype/torro-design)**
(privat). Es enthält Logo, Hörner-Signet, Kampfbulle, Farben, Typografie und
einen interaktiven Design-Guide (`design-guide.html`).

**Bei jeder Design-Änderung gilt: erst dort nachsehen, dann umsetzen.** Farben,
Logo-Varianten oder Schnitte nicht raten und nicht aus dem Bestand der App
ableiten — die App ist nicht die Quelle, `torro-design` ist es.

### ⚠️ Lizenz-Grenze: dieses Repo ist öffentlich

`mahype/torro-design` ist **privat**, `mahype/TorroWhisper` ist **öffentlich**.
Daraus folgt eine harte Regel:

- **Keine Schriftdateien aus `torro-design` in dieses Repo übernehmen.**
  *Frutiger LT* und *Minion Pro* sind kommerziell lizenziert (Linotype/Monotype
  bzw. Adobe) und ausdrücklich nicht zur Weitergabe bestimmt. Sie hier
  einzuchecken wäre eine Lizenzverletzung.
- Für Text in der App deshalb **System-Fonts** (SF Pro / `.system`) verwenden.
  Frutiger ist der Marken-Font für Logo und Kommunikationsmaterial, nicht für
  UI-Fließtext einer Open-Source-App.
- **Grafik-Assets** (Logo, Hörner, Bulle) sind Eigenassets aus „Torro Forms" und
  dürfen übernommen werden. Übernommene Vektoren liegen unter
  [`apps/torrowhisper-macos/Resources/Brand/`](apps/torrowhisper-macos/Resources/Brand/).
- Das Bull-Motiv basiert auf einer lizenzierten iStock-Illustration — nicht in
  neue Kontexte weiterlizenzieren.

### Farben

Maschinenlesbar in `tokens/tokens.json` des Design-Repos, in Swift gespiegelt in
[`TorroBrand.swift`](apps/torrowhisper-macos/Sources/TorroWhisper/TorroBrand.swift).

| Rolle | HEX | Swift | Einsatz |
|---|---|---|---|
| **Torro Rot** | `#D50C0C` | `Color.torroRed` / `.torroAccent` | Kernfarbe, Akzent, Grundflächen |
| Rot (dunkel) | `#A50A0A` | `Color.torroRedDeep` | Verläufe, Hover, Tiefe |
| Schwarz | `#0E0E0F` | `Color.torroBlack` | Kontur (Bulle), Text |
| Silber | `#C4C3C3` | `Color.torroSilver` | Wortmarken-Zusatz |
| Weiß | `#FFFFFF` | `.white` | Schrift auf Rot |

- **`#D50C0C` ist verbindlich.** Nicht verschieben, nicht entsättigen, nicht
  „für Dark Mode anpassen". Der Rotton ist in beiden Appearances derselbe.
- In der App **nicht `Color.accentColor`** verwenden — das ist der
  System-Akzent (blau) und nicht die Marke. Immer `Color.torroAccent`.
- `Color.red` (System-Rot) bleibt für Status/Fehler-Semantik reserviert und ist
  bewusst *nicht* das Marken-Rot — die beiden nicht vermischen.

### Typografie

- Hausschrift der Marke ist **Frutiger LT**, Logo-Schnitt **95 UltraBlack**.
- **In dieser App wird sie nicht ausgeliefert** (siehe Lizenz-Grenze). UI-Text
  nutzt die System-Schrift; Marken-Wirkung entsteht hier über Farbe und Signet,
  nicht über den Font.
- Sekundärschrift der Marke: Minion Pro (Print/Fließtext) — für diese App
  irrelevant.

### Logo-Einsatz

| Element | Datei | Hinweis |
|---|---|---|
| App-Icon | `Resources/Brand/torrowhisper-icon.svg` | Produkt-Icon: Hörner oben, Pegel-Glyph darunter, weiß auf rotem Verlauf (Design-Guide 07/08) |
| Hörner-Signet | `Resources/Brand/horns-{white,red,black}.svg` | kleine Akzente, Loader, Wasserzeichen |
| Grund-Quadrat | `Resources/Brand/torro-logo-square.svg` | nur Hörner auf Rot — Familien-Grundform, **nicht** das App-Icon |
| Wortmarke | nur im Design-Repo | „TORROFORMS" — gehört zur Forms-Marke, **nicht** in diese App |

**So:**
- Das volle Rot als Grund nutzen — die Marke ist für Rot gebaut.
- Auf hellem Grund die rote Fassung von Signet/Wortmarke einsetzen.
- Für kleine Größen das **Hörner-Signet** statt einer gestauchten Wortmarke.
- Freiraum: mindestens eine Hornhöhe rundherum.
- Vektoren verwenden (Outlines), keine gerasterten Logos.

**Nicht:**
- Rotton verschieben oder entsättigen.
- Logo verzerren, neigen oder die Schnittstärke wechseln.
- Weißes Signet auf hellen Grund setzen (unlesbar).
- Schlagschatten oder Effekte auf das flache Logo legen.

### Umsetzung in dieser App

- **Signet:** [`TorroHorns`](apps/torrowhisper-macos/Sources/TorroWhisper/TorroBrand.swift)
  ist eine SwiftUI-`Shape`, aus `horns-white.svg` konvertiert. Als Shape statt
  Bild, damit sie über `foregroundStyle` einfärbbar ist (die Varianten
  weiß/rot/schwarz sind dieselbe Geometrie) und ohne Resource-Plumbing durch das
  `.app`-Bundle auskommt. Natürliches Seitenverhältnis ≈ 1.73:1 → mit
  `.aspectRatio(contentMode: .fit)` layouten.
- **Pegel-Glyph:** [`TorroWaveform`](apps/torrowhisper-macos/Sources/TorroWhisper/TorroBrand.swift)
  ist der Funktions-Glyph der App (fünf gerundete Balken), als Shape aus dem
  24×24-Glyph in `torrowhisper-icon.svg` konvertiert — analog zum Signet
  einfärbbar und ohne Resource-Plumbing.
- **Logo-Kachel:** `TorroLogoTile` — Hörner-Signet über dem Pegel-Glyph auf rotem
  Verlaufs-Squircle, für Stellen an denen sich die App selbst repräsentiert
  (Onboarding, „Über"). Beide Elemente werden mit den Maßverhältnissen aus
  `torrowhisper-icon.svg` platziert (120-Einheiten-Icon), damit die Kachel das
  App-Icon in klein *ist* und kein Nachbau.
- **App-Icon:** wird von [`scripts/generate-app-icon.swift`](scripts/generate-app-icon.swift)
  aus `torrowhisper-icon.svg` erzeugt (`swift scripts/generate-app-icon.swift`
  im Repo-Root). Das Skript rendert den Vektor unverändert und legt nur die
  macOS-Squircle-Maske darüber. Icon-Änderungen gehen über das Skript, nicht
  über manuell bearbeitete `.icns`.
- **Menüleisten-Symbol:** bleibt das SF-Symbol `megaphone` / `megaphone.fill`
  als Template-Image (`AppDelegate.statusImage`). **Nicht durch das Logo
  ersetzen.** macOS erwartet in der Menüleiste einachsige Template-Symbole, die
  sich der Appearance und Tint anpassen; ein rotes Logo bräche dort aus und wäre
  bei 16 pt zudem unleserlich. Das ist eine bewusste Entscheidung — nicht
  „korrigieren".

### Assets aktualisieren

Ändert sich etwas im Design-Repo:

```bash
gh repo clone mahype/torro-design /tmp/torro-design -- --depth 1
cp /tmp/torro-design/logo/horns/horns-*.svg \
   /tmp/torro-design/logo/icon-square/torro-logo-square.svg \
   /tmp/torro-design/logo/icon-square/products/torrowhisper-icon.svg \
   apps/torrowhisper-macos/Resources/Brand/
swift scripts/generate-app-icon.swift
```

Danach `TorroBrand.swift` gegen `tokens/tokens.json` abgleichen; wenn sich die
Hörner-Geometrie geändert hat, `TorroHorns` neu aus dem SVG konvertieren.
**Keine Fonts mitkopieren.**

### Design-Entscheidungen gehören in den Guide

Stellt sich beim Umsetzen heraus, dass eine Guide-Vorgabe hier nicht passt, wird
**nicht hier eine Ausnahme notiert**, sondern die Regel in `app-design.md` im
Repo [`mahype/torro-design`](https://github.com/mahype/torro-design) präzisiert —
sonst driften die Apps der Familie auseinander und jede sammelt ihre eigenen
Sonderfälle. Der Guide ist die Quelle, diese Datei verweist nur darauf.

Auf diesem Weg bereits im Guide geschärft (dort nachlesen, nicht hier):

- **Wizard / Sheet** — der feste Rahmen und „Abbrechen" gelten für Sheets, die
  *committen*; Flächen, die direkt ins Model schreiben, tragen nur „Fertig" und
  werden nach Inhalt bemessen.
- **Schritt-Zeile (Onboarding)** — als Navigations-Rail bleibt der Titel
  `subheadline`.
- **Übernehmen in eine neue App** — `swift/TorroBrandUI.swift` im Design-Repo ist
  älter als die Prosa (Hero noch als Karte); bei Konflikt gilt der Text.
  Bausteine ohne Aufrufer sind Absicht.
