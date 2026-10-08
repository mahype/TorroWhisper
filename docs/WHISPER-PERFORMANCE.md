# Whisper-Performance: Messnachweis zu #43

Messung vom **08.10.2026** auf einem **Apple M4 Max (Mac16,5), 128 GiB RAM,
macOS 26.6.2**, Rust 1.96.0, whisper-rs 0.16.0 / whisper.cpp 1.8.3.
Der Code für Metal, Modellcache, Warmup und Phasenmessung ist bereits seit
0.6.0 vorhanden. Dieser Bericht ergänzt den fehlenden reproduzierbaren Nachweis.

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
  --output /tmp/whisper-results --repetitions 3
```

Das Zielverzeichnis muss neu sein. Während der Messung nicht diktieren, andere
Benchmarks oder Builds ausführen. GPU-Auswahl in den `.log`-Dateien kontrollieren;
`backend_requested` im JSON allein beweist keinen erfolgreichen Metal-Backendstart.

Eine einzelne Konfiguration (Modell, Backend, Threads, Wiederholungen, Sprache,
Segmentierung):

```bash
target/release/examples/whisper_benchmark large_v3_turbo_q5_0 metal 6 3 de multi
```

Die [Rohdaten](benchmarks/2026-10-08-m4-max/summary.json) enthalten alle
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
