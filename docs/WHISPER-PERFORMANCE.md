# Whisper-Performance: Messnachweise zu #43, #88 und #92

Messung vom **08.10.2026** auf einem **Apple M4 Max (Mac16,5), 128 GiB RAM,
macOS 26.6.2**, Rust 1.96.0, whisper-rs 0.16.0 / whisper.cpp 1.8.3.
Der Code für Metal, Modellcache, Warmup und Phasenmessung ist bereits seit
0.6.0 vorhanden. Dieser Bericht ergänzt den fehlenden reproduzierbaren Nachweis.

> Die Metal-Zahlen dieses Abschnitts sind ohne Flash Attention gemessen. Seit #88
> ist Flash Attention auf Apple Silicon eingeschaltet; die neue Messreihe steht im
> Abschnitt [Flash Attention und audio_ctx (#88)](#flash-attention-und-audio_ctx-88).

## Ergebnis und Standardeinstellungen

Für den festen **13,58125 Sekunden** langen deutschen Referenzclip reduziert
Metal die warme Transkriptionszeit von Turbo Q5_0 von **1,927 s auf 0,370 s**
(**5,21×**, rund **81 % weniger Wartezeit**). Die sechs Threads sind in beiden
Fällen identisch. Die Zeit umfasst State-Erzeugung, Inferenz und Textauslesen.

Die Messreihe begründet folgende Entscheidungen:

- **Metal bleibt auf macOS aktiv.** Die Backend-Logs bestätigen echte
  Metal-Inferenz auf dem M4 Max; ein einkompiliertes Feature allein wäre kein Nachweis.
- **Auto-Threads bleiben `min(verfügbare Threads, 6)`**, Fallback 4.
  Sechs Threads liegen bei Q5_0 nur 0,03 % und bei FP16 0,62 % hinter dem besten
  gemessenen Median. Acht Threads bringen keinen praktisch relevanten Vorteil.
  Es gibt keinen Grund, den Default zu erhöhen oder auf diesem einen Gerät eine
  automatische Kalibrierung bei jedem Start einzuführen. Die Experteneinstellung
  bleibt für andere Hardware nutzbar; der Intel-/CPU-Default ist hier nicht neu validiert.
- **Q5_0 bleibt eine sinnvolle speichersparende Whisper-Auswahl.** FP16 und Q5_0
  sind hier mit Metal praktisch gleich schnell. Eine frühere qualitative Aussage
  „FP16 ist schneller“ lässt sich nicht als allgemeine Empfehlung bestätigen.
  Parakeet Ultra bleibt die separate Apple-Silicon-Empfehlung; es wurde nicht gemessen.
- **Feste Sprache empfehlen, wenn sie bekannt ist.** Für diesen deutschen Clip
  braucht FP16 mit `de` 0,370 s, mit automatischer Erkennung 0,622 s (40,5 % weniger
  mit fester Sprache). Die App darf mehrsprachigen Nutzern deshalb nicht automatisch
  Deutsch vorgeben; ihre gespeicherte Sprachwahl bleibt maßgeblich.
- **`single_segment` bleibt aus.** 0,366 s statt 0,370 s sind nur rund 1 % Gewinn.
  Ein sauberer kurzer Clip belegt keine gleichbleibende Qualität langer Diktate.
- **Cache und Hintergrund-Warmup beibehalten.** Modellladen und erste Inferenz
  werden getrennt erfasst; die warmen Wiederholungen laden das Modell nicht erneut.
  Zusätzliche Audio-/Resampler-Umbauten sind durch diese Messung nicht gerechtfertigt.

## CPU gegen Metal

Median von drei warmen Wiederholungen, sechs Threads, feste Sprache `de`,
Greedy-Decoding und die übrigen produktiven Parameter unverändert.
„Small“ ist das Whisper-Small-Modell, intern Preset `standard` (UI: „Medium“).

| Modell | CPU | Metal | Beschleunigung | RTF Metal¹ | Wortfehler² |
|---|---:|---:|---:|---:|---:|
| Turbo Q5_0 | 1,927 s | 0,370 s | 5,21× | 0,0262 | 0 % |
| Turbo FP16 | 2,022 s | 0,370 s | 5,46× | 0,0263 | 0 % |
| Small FP16 | 0,601 s | 0,219 s | 2,74× | 0,0153 | 0 % |

¹ RTF = reine Inferenzzeit / Audiolänge; die CPU-/Metal-Spalten enthalten zusätzlich
State-Erzeugung und Textauslesen. Ein kleinerer RTF ist besser.

² Wortfehlerrate mittels geordneter Editierdistanz nach Kleinschreibung und
Entfernung von Satzzeichen. Einfügen, Löschen, Ersetzen und falsche Wortreihenfolge
zählen. **0 % auf diesem synthetischen Clip ist kein allgemeiner Qualitätsnachweis.**

## Threads mit Metal

Warmer Median in Sekunden; darunter sind alle Einzelwerte in den Rohdaten enthalten.

| Threads | Turbo Q5_0 | Turbo FP16 |
|---:|---:|---:|
| 1 | 0,3804 | 0,3807 |
| 2 | 0,3738 | 0,3742 |
| 4 | 0,3714 | 0,3686 |
| 6 | 0,3697 | 0,3703 |
| 8 | 0,3696 | 0,3680 |

Die Spannweite der drei Wiederholungen bei sechs Threads beträgt 0,3687–0,3737 s
(Q5_0) bzw. 0,3683–0,3709 s (FP16). Der Unterschied zwischen vier, sechs und acht
Threads ist sehr klein. Alle Durchläufe liefern denselben korrekten Wortlaut.

## Laden, erster Durchlauf und Speicher

Metal, sechs Threads, `de`, mehrere Segmente:

| Modell | Laden | Erster Durchlauf³ | Warm | RSS-Zuwachs beim Laden⁴ |
|---|---:|---:|---:|---:|
| Turbo Q5_0 | 0,177 s | 0,383 s | 0,370 s | 647 MiB |
| Turbo FP16 | 0,426 s | 0,387 s | 0,370 s | 1727 MiB |
| Small FP16 | 0,188 s | 0,292 s | 0,219 s | 580 MiB |

³ Frischer Prozess/Kontext, **kein Kaltstart nach Rechnerneustart**. Dateisystem-
und Treiber-Caches wurden nicht geleert. Die allererste Q5_0-CPU-Konfiguration
brauchte 5,797 s zum Laden; sogar bei `use_gpu=false` enumeriert whisper.cpp die
Backends und initialisiert dabei die Metal-Bibliothek. Dieser einmalige Ausreißer
wird offen in den Rohdaten belassen, aber nicht als Metal-Beschleunigung verkauft.
Der erste Q5_0-Metal-Lauf (ein Thread) brauchte 0,777 s statt warmer 0,380 s.

⁴ Differenz des Prozess-RSS vor/nach Kontextaufbau. Kein Peak während der Inferenz
und keine vollständige Bilanz des gemeinsam verwendeten GPU-Speichers.

## Messverfahren und Grenzen

Das Beispielprogramm `whisper_benchmark` lädt Modelle direkt und ruft
`dictation::run_whisper_inference` auf, also denselben Decode-Pfad wie das Diktat.
Es startet keine App-Runtime, registriert keine Hotkeys und schreibt keine Settings.
Es verwendet den vorhandenen, mit `say -v Anna` erzeugten Referenzclip
`crates/torrowhisper-bridge/resources/benchmark-de.wav` (16 kHz Mono-PCM).

Jede der 16 Konfigurationen läuft in einem neuen Prozess: zuerst eine separat
ausgewiesene Inferenz, danach drei Wiederholungen mit demselben Modellkontext und
jeweils neuem Whisper-State. Die Threadreihenfolge wird zwischen den zwei
Turbo-Modellen umgedreht. Zwischen Konfigurationen liegen zwei Sekunden Pause.
Gemessen wurde in einer normalen Desktop-Sitzung; dies ist keine vollständig
isolierte Hardware-Laborumgebung und keine statistische Studie.

**Vorher/Nachher bedeutet hier CPU- gegen Metal-Ausführung desselben aktuellen
Release-Builds** (`use_gpu=false/true`). CPU darf Accelerate/BLAS nutzen.
Es ist kein nachgebauter historischer Release. Das isoliert den Effekt der
Hardwarebeschleunigung von anderen zwischenzeitlichen Änderungen.

Die Backend-Logs wurden für **alle 16 Konfigurationen** geprüft: CPU meldet
`use gpu = 0` und `no GPU found` (absichtlich deaktiviert); Metal meldet
`use gpu = 1`, `GPU name: Apple M4 Max` und `using Metal backend`.
Die relevanten, deduplizierten Zeilen liegen in
[backend-evidence.txt](benchmarks/2026-10-08-m4-max/backend-evidence.txt).

**Diese Zahlen messen Whisper, nicht die vollständige Zeit bis zum Einfügen.**
Live-Aufnahme, VAD-Kürzung, MP3-Export, optionale LLM-Nachbearbeitung und Paste
sind nicht Teil des Vergleichs. Deren tatsächliche Zeiten werden bereits von
der App über `StageTimingDto`/Diagnoseansicht einschließlich `total_after_stop_secs`
pro Diktat erfasst. Der Benchmark ergänzt einen kontrollierten Nachweis des
Whisper-Engpasses; er erfindet keine Messung fremder Anwendungen oder LLMs.

Nicht gemessen: Intel-Macs, Windows/Linux, Akkubetrieb, Core ML, lange Audios,
Akzente, Rauschen und mehrere Sprachen. Ein Modell-/Thread-Default für alle
Geräte lässt sich aus diesem einzelnen Rechner nicht ableiten. Die vorhandene
macOS-spezifische Metal-Abhängigkeit und CPU-Pfade anderer Plattformen bleiben erhalten.

## Flash Attention und audio_ctx (#88)

Messung vom **04.11.2026** auf demselben **Apple M4 Max (Mac16,5), 128 GiB RAM**,
jetzt **macOS 27.0.1**, Rust 1.96.0, whisper-rs 0.16.0. Gemessen wurde vom sauberen
Commit `7b47b5c` (`source_dirty=false`), Suite `tuning`, 44 Konfigurationen mit je
einem ersten Durchlauf und **fünf warmen Wiederholungen**. Rohdaten:
[summary.json](benchmarks/2026-11-04-m4-max/summary.json),
[metadata.json](benchmarks/2026-11-04-m4-max/metadata.json),
[backend-evidence.txt](benchmarks/2026-11-04-m4-max/backend-evidence.txt).

Zwei Stellschrauben waren bisher ungenutzt:

- **Flash Attention**: whisper-rs 0.16 erzeugt Kontexte mit `flash_attn=false`,
  obwohl whisper.cpp selbst `true` als Standard hat. DTW wird nicht genutzt, also
  geht durch Flash Attention nichts verloren.
- **`audio_ctx`**: Whisper kodiert immer ein 30-s-Fenster (1500 Positionen), auch
  bei einem 3-s-Diktat. Die Variante `ctx` setzt das Fenster auf die Audiolänge plus
  eine Sekunde (`ceil(Samples / 320) + 50`, ab 1500 wieder das volle Fenster).

Neu ist ein zweiter, kurzer Referenzclip
`crates/torrowhisper-bridge/resources/benchmark-de-short.wav` (2,45 s, „Bitte schick
mir die Unterlagen bis morgen.“, ebenfalls `say -v Anna`, kein Ausschnitt des langen
Clips). Der lange Clip ist der bisherige (13,58 s).

**Regel für die Übernahme** (aus dem angenommenen Plan): Eine Stellschraube gilt als
messbar schneller, wenn sie mit Metal beim kurzen Clip mindestens 5 % schneller ist,
sich die Spannweiten der warmen Wiederholungen nicht überschneiden und der lange
Clip nicht langsamer wird. Sie darf in keiner Messung (auch CPU und automatische
Spracherkennung) mehr Wortfehler machen. Verglichen wird immer mit `aus`/`voll` in
sonst gleicher Konfiguration.

### Ergebnis

Sechs Threads, mehrere Segmente. Median und Spannweite der fünf warmen Durchläufe
einschließlich State-Erzeugung und Textauslesen; RTF = reine Inferenzzeit /
Audiolänge; `max_wer` = schlechtester Wortfehlerwert der fünf Durchläufe
(geordnete Editierdistanz, Werte über 100 % bedeuten erfundene Wiederholungen).
`audio_ctx` „voll“ = whisper.cpp-Standard (0, 30-s-Fenster).

| Modell | Backend | Clip | Sprache | Flash Attention | `audio_ctx` | Median | Spannweite | RTF | `max_wer` |
|---|---|---|---|---|---:|---:|---:|---:|---:|
| Turbo Q5_0 | Metal | kurz | `de` | aus | voll | 0,310 s | 0,308–0,350 s | 0,1193 | 0 % |
| Turbo Q5_0 | Metal | kurz | `de` | an | voll | 0,262 s | 0,260–0,282 s | 0,1022 | 0 % |
| Turbo Q5_0 | Metal | kurz | `de` | aus | 173 | 1,318 s | 1,226–1,357 s | 0,5305 | 200 % |
| Turbo Q5_0 | Metal | kurz | `de` | an | 173 | 1,196 s | 1,145–1,350 s | 0,4843 | 1129 % |
| Turbo Q5_0 | Metal | lang | `de` | aus | voll | 0,388 s | 0,385–0,456 s | 0,0271 | 0 % |
| Turbo Q5_0 | Metal | lang | `de` | an | voll | 0,319 s | 0,317–0,321 s | 0,0226 | 0 % |
| Turbo Q5_0 | Metal | lang | `de` | aus | 730 | 0,234 s | 0,232–0,238 s | 0,0160 | 17 % |
| Turbo Q5_0 | Metal | lang | `de` | an | 730 | 0,203 s | 0,201–0,205 s | 0,0142 | 17 % |
| Turbo Q5_0 | CPU | kurz | `de` | aus | voll | 1,979 s | 1,927–2,030 s | 0,8059 | 0 % |
| Turbo Q5_0 | CPU | kurz | `de` | an | voll | 3,622 s | 3,519–4,020 s | 1,4766 | 0 % |
| Turbo Q5_0 | CPU | kurz | `de` | aus | 173 | 1,320 s | 1,311–1,401 s | 0,5376 | 200 % |
| Turbo Q5_0 | CPU | kurz | `de` | an | 173 | 1,774 s | 1,730–3,010 s | 0,7231 | 1500 % |
| Turbo Q5_0 | CPU | lang | `de` | aus | voll | 2,101 s | 2,065–2,274 s | 0,1542 | 0 % |
| Turbo Q5_0 | CPU | lang | `de` | an | voll | 3,907 s | 3,687–3,935 s | 0,2872 | 0 % |
| Turbo Q5_0 | CPU | lang | `de` | aus | 730 | 0,938 s | 0,909–1,425 s | 0,0686 | 17 % |
| Turbo Q5_0 | CPU | lang | `de` | an | 730 | 1,251 s | 1,244–1,273 s | 0,0917 | 17 % |
| Turbo Q5_0 | Metal | kurz | `auto` | aus | voll | 0,748 s | 0,608–0,777 s | 0,2984 | 0 % |
| Turbo Q5_0 | Metal | kurz | `auto` | an | voll | 0,622 s | 0,580–0,630 s | 0,2504 | 0 % |
| Turbo Q5_0 | Metal | kurz | `auto` | aus | 173 | 1,556 s | 1,496–1,622 s | 0,6293 | 200 % |
| Turbo Q5_0 | Metal | kurz | `auto` | an | 173 | 0,293 s | 0,287–0,301 s | 0,1157 | 100 % |
| Turbo Q5_0 | Metal | lang | `auto` | aus | voll | 0,724 s | 0,649–0,775 s | 0,0517 | 0 % |
| Turbo Q5_0 | Metal | lang | `auto` | an | voll | 0,689 s | 0,683–0,715 s | 0,0500 | 0 % |
| Turbo Q5_0 | Metal | lang | `auto` | aus | 730 | 0,532 s | 0,510–0,581 s | 0,0379 | 17 % |
| Turbo Q5_0 | Metal | lang | `auto` | an | 730 | 0,477 s | 0,464–0,484 s | 0,0344 | 52 % |
| Turbo FP16 | Metal | kurz | `de` | aus | voll | 0,292 s | 0,289–0,296 s | 0,1117 | 0 % |
| Turbo FP16 | Metal | kurz | `de` | an | voll | 0,242 s | 0,241–0,242 s | 0,0948 | 0 % |
| Turbo FP16 | Metal | kurz | `de` | aus | 173 | 1,673 s | 1,556–1,788 s | 0,6754 | 286 % |
| Turbo FP16 | Metal | kurz | `de` | an | 173 | 1,503 s | 1,489–1,548 s | 0,6100 | 800 % |
| Turbo FP16 | Metal | lang | `de` | aus | voll | 0,393 s | 0,383–0,404 s | 0,0272 | 0 % |
| Turbo FP16 | Metal | lang | `de` | an | voll | 0,325 s | 0,315–0,346 s | 0,0232 | 0 % |
| Turbo FP16 | Metal | lang | `de` | aus | 730 | 0,246 s | 0,245–0,250 s | 0,0170 | 17 % |
| Turbo FP16 | Metal | lang | `de` | an | 730 | 0,198 s | 0,196–0,200 s | 0,0139 | 0 % |
| Turbo FP16 | CPU | kurz | `de` | aus | voll | 1,933 s | 1,905–1,950 s | 0,7860 | 0 % |
| Turbo FP16 | CPU | kurz | `de` | an | voll | 3,402 s | 3,374–3,465 s | 1,3879 | 0 % |
| Turbo FP16 | CPU | kurz | `de` | aus | 173 | 2,245 s | 2,192–2,597 s | 0,9154 | 286 % |
| Turbo FP16 | CPU | kurz | `de` | an | 173 | 2,344 s | 2,307–2,403 s | 0,9559 | 800 % |
| Turbo FP16 | CPU | lang | `de` | aus | voll | 2,042 s | 2,015–2,062 s | 0,1496 | 0 % |
| Turbo FP16 | CPU | lang | `de` | an | voll | 3,612 s | 3,597–3,635 s | 0,2656 | 0 % |
| Turbo FP16 | CPU | lang | `de` | aus | 730 | 0,913 s | 0,906–0,918 s | 0,0665 | 17 % |
| Turbo FP16 | CPU | lang | `de` | an | 730 | 1,325 s | 1,282–1,357 s | 0,0972 | 17 % |

### Entscheidungen

- **Flash Attention: übernommen, nur auf Apple Silicon.** Mit Metal und `de` ist der
  kurze Clip 15,6 % (Q5_0, 0,310 → 0,262 s) bzw. 17,2 % (FP16, 0,292 → 0,242 s)
  schneller, die Spannweiten überschneiden sich nicht. Der lange Clip wird ebenfalls
  schneller (17,8 % bzw. 17,3 %). In keiner der zehn Messungen mit Flash Attention
  und vollem Fenster steigt die Wortfehlerrate, alle bleiben bei 0 % (mit verkürztem
  Fenster siehe unten). Mit automatischer Spracherkennung
  ist der Median 16,8 % (kurz) bzw. 4,8 % (lang) niedriger; dort streuen die
  Durchläufe stärker und die Spannweiten überschneiden sich, ein Gegenbefund ist das
  nicht. Auf der CPU des M4 Max ist Flash Attention dagegen **76–86 % langsamer**.
  Deshalb gilt `WHISPER_FLASH_ATTN = cfg!(all(target_os = "macos", target_arch =
  "aarch64"))`: Intel-Macs, die nur die CPU nutzen und hier nicht messbar sind,
  bleiben unverändert. Die Backend-Logs bestätigen für alle 20 Läufe
  `flash attn = 1`, für die übrigen 24 `flash attn = 0`.
- **Angepasstes `audio_ctx`: verworfen.** Beim kurzen Clip (`audio_ctx` 173)
  wiederholen beide Turbo-Modelle den Satz mehrfach (`max_wer` 200–1500 %), mit
  Metal und `de` wird es dadurch sogar vier- bis sechsmal langsamer. Mit automatischer
  Spracherkennung und Flash Attention liefert Q5_0 nur „of“ (100 %), offenbar eine
  falsch erkannte Sprache. Beim langen Clip (`audio_ctx` 730) ist ein Durchlauf mit `de` zwar 35–55 %
  schneller, aber sieben der acht Varianten mit `de` hängen „Die Privatsphäre ist jederzeit
  gewährleistet.“ ein zweites Mal an (17 %), mit automatischer Spracherkennung und
  Flash Attention wird ein ganzer Satz wiederholt (52 %). Die Turbo-Modelle sind auf das volle 30-s-Fenster
  trainiert; ein kürzeres Fenster verletzt die WER-Regel. Zusammen mit Flash Attention
  wird es beim kurzen Clip mit `de` noch schlechter (bis 1500 %): whisper.cpp rechnet die
  Cross-Attention dann über ein auf 256 aufgerundetes Fenster ohne Maske. Der Schalter
  `WHISPER_DYNAMIC_AUDIO_CTX` bleibt aus, die Funktion `audio_ctx_for` und die
  Benchmark-Variante bleiben für künftige Modelle erhalten.
- **Flash Attention auf den übrigen Presets (Nachmessung vom 05.11.2026).** Der
  Schalter gilt auf Apple Silicon für alle Whisper-Presets. Deshalb wurden zusätzlich
  Small (`standard`) und Large v3 mit Metal, `de` und vollem Fenster gemessen (Suite
  `presets`, sauberer Commit `78ae2a2`, fünf warme Wiederholungen;
  [summary.json](benchmarks/2026-11-05-m4-max-presets/summary.json),
  [backend-evidence.txt](benchmarks/2026-11-05-m4-max-presets/backend-evidence.txt)):

  | Modell | Clip | ohne FA | mit FA | Gewinn | `max_wer` |
  |---|---|---:|---:|---:|---:|
  | Small | kurz | 0,103 s (0,100–0,129) | 0,078 s (0,077–0,079) | 24,3 % | 0 % → 0 % |
  | Small | lang | 0,221 s (0,217–0,223) | 0,180 s (0,179–0,183) | 18,6 % | 0 % → 0 % |
  | Large v3 | kurz | 0,420 s (0,418–0,421) | 0,358 s (0,355–0,363) | 14,8 % | 0 % → 0 % |
  | Large v3 | lang | 0,860 s (0,854–0,939) | 0,723 s (0,717–0,781) | 15,9 % | 0 % → 0 % |

  Beide erfüllen die Regel: kurz mindestens 5 % schneller ohne Überschneidung, lang
  ebenfalls schneller, keine zusätzlichen Wortfehler. Tiny, Base und Medium wurden
  nicht gemessen; sie teilen die Architektur, der Schalter gilt für sie mit.
- **Kombination**: entfällt, weil nur Flash Attention übernommen wird.
- **Threads der Live-Vorschau: auf Apple Silicon zwei.** Gemessen mit Metal, kurzem
  Clip, `de`, ohne Flash Attention und mit vollem Fenster:

  | Modell | Threads | Median | Spannweite | RTF | `max_wer` |
  |---|---:|---:|---:|---:|---:|
  | Turbo Q5_0 | 2 | 0,305 s | 0,303–0,306 s | 0,1187 | 0 % |
  | Turbo Q5_0 | 4 | 0,315 s | 0,314–0,323 s | 0,1216 | 0 % |
  | Turbo FP16 | 2 | 0,305 s | 0,294–0,344 s | 0,1181 | 0 % |
  | Turbo FP16 | 4 | 0,380 s | 0,353–0,434 s | 0,1469 | 0 % |

  Damit braucht Q5_0 mit zwei Threads 0,305 s, mit vier 0,315 s
  (Verhältnis 0,97); FP16 0,305 s gegen 0,380 s (0,80, die Vier-Thread-Reihe streut
  hier mit 0,353–0,434 s). Zwei Threads sind also bei beiden Modellen nicht langsamer
  als vier (Regel: Median ≤ 1,05 × vier Threads). `streaming_thread_count` gibt auf
  Apple Silicon deshalb 2 zurück, andere Plattformen behalten `Kerne / 2` (2–4). Der
  finale Durchlauf bleibt bei seiner Einstellung (Standard sechs Threads). Die
  Thread-Reihe lief wie geplant ohne Flash Attention; ob zwei Threads auch mit Flash
  Attention nicht langsamer sind, ist nicht eigens gemessen. Der End-to-End-Test der
  Live-Vorschau mit Flash Attention und zwei Threads liefert den ersten Text nach 0,57 s.

### Grenzen

Beide Clips sind synthetische Sprache einer Stimme ohne Rauschen. Flash Attention
ändert die Rechenreihenfolge, nicht das Modell; ein Qualitätsnachweis für echte
Diktate ist dieser Clip trotzdem nicht. Die CPU-Messung läuft auf dem M4 Max und
sagt wenig über Intel-Macs aus. Fällt auf Apple Silicon der Metal-Start aus und
whisper.cpp rechnet auf der CPU, bleibt Flash Attention trotzdem an und macht diesen
seltenen Notfallpfad langsamer. Gemessen wurde nur Whisper, nicht Aufnahme,
VAD-Kürzung, Nachbearbeitung und Einfügen.

## Final-Pass bei langen Takes (#92)

Bei Aufnahmen über 30 s fehlte im eingefügten Text ein großer Teil, und Teile
standen doppelt da. Ursache: Mit `set_no_timestamps(true)` springt whisper.cpp
nach jedem 30-s-Fenster fest 30 s weiter (`seek_delta` im Zweig
`single_segment || no_timestamps` von `whisper_full_with_state`). Hört der
Decoder im Fenster früher auf oder gerät in eine Wiederholung, fällt der Rest
des Fensters weg. Mit Zeitstempeln springt whisper.cpp nur bis zum letzten
erkannten Zeitstempel. `run_whisper_inference` schaltet deshalb für Takes über
30 s `no_timestamps` und `single_segment` aus (`needs_window_timestamps`);
kürzere Takes bleiben unverändert. Live-Vorschau und Warmup nutzen weiter
`base_full_params`.

Messung auf dem M4 Max, Turbo Q5_0, Clips mit `say -v Anna` bei 48 kHz,
Wortfehlerrate (WER) und Inferenzzeit, vorher / nachher:

| Clip | Sprache | WER vorher | WER nachher | Inferenz vorher | Inferenz nachher |
|---|---|---|---|---|---|
| 13,8 s | de | 0,000 | 0,000 | 0,5 s | 0,5 s |
| 13,8 s | auto | 0,000 | 0,000 | 1,2 s | 1,2 s |
| 41 s mit Pausen | de | 0,146 | 0,011 | 1,2 s | 1,8 s |
| 41 s mit Pausen | auto | 0,146 | 0,011 | 1,8 s | 2,3 s |
| 163 s ohne Pause | de | 0,620 | 0,000 | 4,0 s | 6,0 s |
| 163 s ohne Pause | auto | 0,620 | 0,000 | 4,3 s | 6,0 s |

Aufruf (alle drei Variablen sind Pflicht, der Test bricht bei WER ≥ 0,05 ab):

```bash
OW_TEST_MODEL=~/Library/Application\ Support/com.gettorro.TorroWhisper/models/ggml-large-v3-turbo-q5_0.bin \
OW_TEST_WAV48=/tmp/long.wav OW_TEST_REF_TEXT_FILE=/tmp/long.txt \
cargo test --release -p torrowhisper-bridge final_pass_long_take -- --ignored --nocapture
```

Bekannte Grenze (in der Planung gemessen): Large v3 liegt beim 180-s-Clip nach dem Fix bei WER 6,9 %
(vorher 68 %), also über der Zielmarke; dieser Rest ist nicht Teil von #92.

## Reproduzieren

Voraussetzung: macOS, Rust/CMake/Swift wie für den regulären Bridge-Build,
Python **3.11+**, heruntergeladene Modelle `large_v3_turbo_q5_0`,
`large_v3_turbo` und `standard`. Der Runner lädt nichts automatisch herunter.

```bash
# Aktuelles macOS: Swift-Runtime für die Bridge wie im CI erreichbar machen.
RUSTFLAGS='-C link-arg=-Wl,-rpath,/usr/lib/swift' \
  cargo build --release -p torrowhisper-bridge --example whisper_benchmark

python3 scripts/benchmark-whisper.py \
  --binary target/release/examples/whisper_benchmark \
  --output /tmp/whisper-results --repetitions 5 --suite tuning
```

`--suite tuning` (Standard) misst die #88-Varianten (Flash Attention × `audio_ctx` ×
Clip, siehe oben), `--suite presets` Flash Attention auf Small und Large v3. `--suite baseline` wiederholt den #43-Vergleich (CPU/Metal,
Threads 1–8, Small, automatische Sprache, ein Segment), jeweils ohne Flash Attention,
mit vollem Fenster und dem langen Clip.

Das Zielverzeichnis muss neu sein. Während der Messung nicht diktieren, andere
Benchmarks oder Builds ausführen. GPU-Auswahl in den `.log`-Dateien kontrollieren;
`backend_requested` im JSON allein beweist keinen erfolgreichen Metal-Backendstart.

Eine einzelne Konfiguration (Modell, Backend, Threads, Wiederholungen, Sprache,
Segmentierung, Flash Attention, `audio_ctx`, Clip):

```bash
target/release/examples/whisper_benchmark large_v3_turbo_q5_0 metal 6 5 de multi fa noctx short
```

Die Varianten der letzten drei Argumente sind `fa|nofa`, `ctx|noctx` und
`long|short`; sie gelten nur für diesen Lauf und unabhängig von den Schaltern der App.

Die [Rohdaten zu #43](benchmarks/2026-10-08-m4-max/summary.json) enthalten alle
Einzelmessungen, Transkripte, Phasen, Qualitätswerte und eine Zusammenfassung.
[metadata.json](benchmarks/2026-10-08-m4-max/metadata.json) enthält Hardware,
Basis-Commit, SHA-256 von Modellen, Testaudio, Binärdatei und Messquellen. Der
Messcode lag beim Lauf als Diff auf `f53c192` vor (`source_dirty=true`); die
Quell-Hashes halten diesen Zustand nachvollziehbar fest.

Tests: 129 Bridge-Unit-Tests bestanden (3 bestehende ignoriert), darunter die
neuen Fälle für Wortfehler und ungültige Benchmark-Anfragen. Der separate
FFI-Smoke-Test verlangt jetzt echte positive Inferenzzeiten und einen nichtleeren
Text; reine Platzhalter für fehlende Modelle zählen nicht mehr als Erfolg.
Dieser Smoke-Test ist mit den lokal installierten Modellen bestanden. Ebenso
erfolgreich: Release-Build des Beispiels, `cargo clippy --release -p
torrowhisper-bridge --all-targets -- -D warnings`, Format- und Diff-Prüfung.
Ein erneuter Swift-/Universal-App-Build war nicht Teil dieser Messung;
Swift, die C-FFI-Schnittstelle und die plattformspezifischen Dependencies wurden
nicht geändert.
