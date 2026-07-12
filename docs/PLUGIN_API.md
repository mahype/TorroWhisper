# Plugin-API

Dieses Dokument beschreibt die Schnittstelle, über die Plugins mit der App
sprechen — insbesondere wie ein Plugin die **verfügbaren Sprachmodelle**
auflistet und nutzt. Bezugspunkt ist GitHub #15 (Plugin-System).

> **Status.** Das Chat-Plugin (#17), die erste Referenz-Implementierung, wurde
> mit #34 vorerst ausgebaut (Fokus Diktierfunktion; Reaktivierung über Branch
> `feat/chat-plugin` möglich). Der hier beschriebene **stabile Vertrag** gilt
> unverändert — aktueller Konsument ist die LLM-Nachbearbeitungs-Stage der
> Pipeline; in Phase 2 nutzen ihn auch Dritt-Plugins. Wer ein neues eingebautes
> Plugin oder eine Pipeline-Stage schreibt, programmiert ausschließlich gegen
> diesen Vertrag.

---

## 1. Überblick

Ein Plugin bekommt vom Host genau drei Dinge — gebündelt im Trait
[`PluginHost`](../crates/torrowhisper-bridge/src/plugin_api.rs):

1. **Modelle entdecken** — die einheitliche Modell-Registry (lokale Presets,
   eigene GGUF-Modelle, Cloud-Modelle), jeweils mit Verfügbarkeits-Status.
2. **Modell ausführen** — wahlweise dialogisch (`chat`) oder als
   Anweisung/Umschreibung (`generate`).
3. **Loggen** — einheitlich unter dem Ziel `plugin:<id>`.

Ein Plugin greift **nur** über diesen Trait auf den Host zu, nie an ihm vorbei in
Bridge-Interna. Dadurch bleibt die Grenze versionierbar.

```
Plugin  ──►  PluginHost (Trait)  ──►  llm::provider_for  ──►  Backend
  (Pipeline-     ├─ available_models / ready_models        (lokales GGUF-Helper-
   Stage, …)     ├─ chat / generate                         Subprocess, Ollama,
                 └─ log                                     LM Studio, Cloud)
```

Der konkrete Host ist
[`BridgeHost`](../crates/torrowhisper-bridge/src/plugin_api.rs). Er hält einen
**Settings-Snapshot** und ist damit `Send` — er kann in den Arbeits-Thread eines
Plugins wandern (z. B. läuft die Nachbearbeitung außerhalb der Hauptschleife).

---

## 2. Der Vertrag: `PluginHost`

Definiert in `crates/torrowhisper-bridge/src/plugin_api.rs`.

```rust
pub const PLUGIN_API_VERSION: u32 = 1;

pub enum LogLevel { Debug, Info, Warn, Error }

pub trait PluginHost {
    /// API-Version, die dieser Host implementiert.
    fn api_version(&self) -> u32 { PLUGIN_API_VERSION }

    /// Alle Modelle der Registry, je mit Verfügbarkeit.
    fn available_models(&self) -> Vec<LlmRegistryEntryDto>;

    /// Nur sofort lauffähige Modelle (heruntergeladen / Key gesetzt).
    fn ready_models(&self) -> Vec<LlmRegistryEntryDto>;

    /// Dialogisch: system_prompt wird unverändert als System-Nachricht
    /// genutzt, user_text als Nutzer-Turn — das Modell antwortet.
    fn chat(&self, model: &LlmModelRef, system_prompt: &str, user_text: &str,
            cancelled: &Arc<AtomicBool>) -> Result<String, String>;

    /// Anweisend: role_prompt rahmt eine Transform-/Umschreib-Aufgabe über
    /// user_text (Nachbearbeitungs-Stil).
    fn generate(&self, model: &LlmModelRef, role_prompt: &str, user_text: &str,
                cancelled: &Arc<AtomicBool>) -> Result<String, String>;

    /// Loggt unter plugin:<id>.
    fn log(&self, level: LogLevel, message: &str);
}
```

### Host bauen

```rust
use torrowhisper_bridge::plugin_api::{BridgeHost, PluginHost, LogLevel};

// settings: AppSettings — ein Snapshot, der in den Plugin-Thread wandern darf.
let host = BridgeHost::new("mein_plugin", settings.clone());
```

`"mein_plugin"` ist die Plugin-ID. Sie taucht in jeder Log-Zeile als
`plugin:mein_plugin` auf (die LLM-Nachbearbeitungs-Stage nutzt z. B.
`post_processing`).

---

## 3. Modelle entdecken: die Registry

`available_models()` liefert dieselbe Liste, die auch die Einstellungs-UI zeigt —
zusammengebaut in `crates/torrowhisper-bridge/src/llm/registry.rs`. Sie enthält:

- **Lokale Presets** — Gemma-Varianten (`LlmPreset::Small/Medium/Large`).
- **Eigene GGUF-Modelle** — alles aus `AppSettings.custom_llm_models`.
- **Cloud-Modelle** — kuratierte Defaults pro Anbieter (Claude, GPT, Mistral,
  DeepSeek, Grok, Gemini).

Jeder Eintrag ist ein `LlmRegistryEntryDto`
(`crates/torrowhisper-core/src/lib.rs`):

| Feld | Bedeutung |
|------|-----------|
| `stable_id` | Kanonisches Auswahl-Token, z. B. `local_preset:medium`, `anthropic:claude-opus-4-8`. |
| `model_ref` | Backend-konkrete Referenz (`LlmModelRef`) — wird an `chat`/`generate` übergeben. |
| `backend_kind` | Taxonomie (`LocalGguf`, `Ollama`, `OpenAi`, `Anthropic`, `Gemini`, …). |
| `display_name` | Anzeigename, z. B. „Gemma 4 Medium“, „GPT-4o mini“. |
| `detail` | Zweite Zeile: Größe / Quant / „Cloud · needs API key“. |
| `availability` | Siehe Tabelle unten. |
| `progress_basis_points` | Download-Fortschritt 0..=10000 (nur bei `Downloading`). |

### Verfügbarkeit (`LlmAvailability`)

| Wert | Bedeutung | Nutzbar? |
|------|-----------|----------|
| `Ready` | Lokale Datei vorhanden + valide, **oder** Cloud-Key gesetzt. | ✅ |
| `Downloadable` | Lokales Preset/Custom noch nicht auf Platte. | ❌ (erst laden) |
| `Downloading` | Download läuft (`progress_basis_points`). | ❌ |
| `Corrupt` | Lokale Datei besteht den GGUF-Integritätscheck nicht. | ❌ |
| `NeedsApiKey` | Cloud-Backend ohne hinterlegten API-Key. | ❌ (erst Key) |

**Empfehlung:** Für ein Auswahl-Menü `available_models()` nehmen und den Status
anzeigen (Nicht-`Ready` markieren oder ausgrauen). Soll nur das Lauffähige
erscheinen, `ready_models()` verwenden.

> **Hinweis.** `BridgeHost::available_models()` baut die Registry über
> `registry::build_static()` — ohne den laufenden Download-Manager. Folge: ein
> gerade laufender Download wird nicht als `Downloading` gemeldet (er erscheint
> als `Downloadable`/`Ready` je nach Dateizustand). Alle anderen Zustände stimmen.
> Die Live-Variante mit Fortschritt nutzt nur die UI über `ow_get_llm_registry`.

---

## 4. Modell ausführen: `chat` vs. `generate`

Beide Methoden lösen `model` über denselben Dispatch auf, den auch die
Nachbearbeitung nutzt (`llm::provider_for`), und rufen dann das passende
Backend (lokaler GGUF-Helper-Subprocess, Ollama, LM Studio, Cloud-HTTP).

- **`chat(model, system_prompt, user_text, cancelled)`** — `system_prompt` wird
  **wörtlich** als System-Nachricht des Assistenten gesetzt, `user_text` ist der
  Nutzer-Turn. Das Modell **antwortet** auf die Eingabe. Seit dem Ausbau des
  Chat-Plugins (#34) ruhende, aber weiterhin gültige Fähigkeit des Vertrags.
- **`generate(model, role_prompt, user_text, cancelled)`** — `role_prompt`
  rahmt eine **Aufgabe** über `user_text` (z. B. „korrigiere diesen diktierten
  Text“). Das Modell **schreibt um**, statt zu antworten. Das nutzt die
  LLM-Nachbearbeitungs-Stage.

`cancelled: &Arc<AtomicBool>` ist eine geteilte Abbruch-Flagge. Setzt der Host
sie auf `true`, brechen lang laufende Generierungen kooperativ ab. Plugins
reichen die Flagge nur durch.

Beide geben `Result<String, String>` zurück — `Ok(antwort)` oder `Err(meldung)`
mit einer für Menschen lesbaren Fehlermeldung (z. B. „Custom language model 'x'
has not been downloaded yet.“ oder „API key for OpenAI is not configured.“).

> **Wichtig — Verfügbarkeit vor dem Aufruf prüfen.** Ein `model_ref` aus
> `available_models()` mit Status ≠ `Ready` lässt sich übergeben, scheitert aber
> zur Laufzeit (nicht geladen / kein Key). Entweder `ready_models()` nutzen oder
> die `availability` vorher prüfen.

---

## 5. Loggen

`host.log(LogLevel::Info, "…")` schreibt unter das Ziel `plugin:<id>` in das eine
gemeinsame App-Log (Implementierung:
`crates/torrowhisper-bridge/src/plugin_log.rs`). So lässt sich später exakt
nachvollziehen, was ein Plugin getan hat oder warum es fehlschlug. Plugins
sollten **nicht** direkt `log::` aufrufen, sondern immer `host.log(...)`.

Von der Swift-Seite loggen Plugins über `BridgeClient.pluginLog(...)` →
FFI `ow_plugin_log` → dieselbe Fassade.

---

## 6. Versionierung

`PLUGIN_API_VERSION` (aktuell `1`) markiert die Vertrags-Version.

- **Erweiterungen** (neue Default-Methode, neues optionales Feld) erhöhen die
  Version **nicht** notwendigerweise.
- **Brechende Änderungen** (geänderte Signatur, entfernte Methode, geänderte
  Semantik) **müssen** `PLUGIN_API_VERSION` erhöhen.

Ein Plugin kann `host.api_version()` prüfen und sich bei Inkompatibilität sauber
verweigern, statt undefiniert zu laufen.

---

## 7. Pipeline-Stages (Nachbearbeitung)

Plugins, die in die **Nachbearbeitungs-Pipeline** einklinken, implementieren
zusätzlich den Stage-Vertrag aus
`crates/torrowhisper-bridge/src/pipeline/mod.rs`:

| Bestandteil | Rolle |
|-------------|-------|
| `PipelineStage` | `id(&self) -> &str` + `run(&self, ctx) -> Result<Outcome, Error>`. |
| `StageFactory` | Erzeugt Stage-Instanzen: `build(&self, cx) -> Box<dyn PipelineStage>`. |
| `StageRegistry` | Hält Factories; `register()` nimmt Plugin-Factories auf. |
| `PipelineContext` | Reise-Objekt: veränderlicher `text`, unveränderlicher `original_transcript`, `vars` (Seitenkanal-Metadaten), `history` (Ausführungs-Log). |

- **Namenskonvention** für Plugin-Stages: `plugin:<id>.<name>` (z. B.
  `plugin:translate.deepl`). Unbekannte Stage-IDs werden **geloggt und
  übersprungen**, nie als harter Fehler behandelt.
- **Konfiguration** je Schritt: `PipelineStepConfig`
  (`crates/torrowhisper-core/src/lib.rs`) trägt `stage_id`, `enabled` und ein
  **opakes JSON-Feld `config`**, das das Plugin frei interpretiert. Der Host
  schaut da nicht hinein.

Innerhalb einer Stage holt man sich Modelle/Generierung über einen `BridgeHost`
(siehe Referenz `LlmStage` unten).

---

## 8. Konfiguration eines Plugins

Zwei Wege, je nach Plugin-Art:

1. **Eigene Settings-Felder** — strukturierte Felder in `AppSettings` (so hielt
   das ausgebaute Chat-Plugin seine Konfiguration unter `AppSettings.chat.*`).
   Geeignet für eingebaute Plugins mit fester UI.
2. **Opakes `config`-JSON** pro Pipeline-Schritt (`PipelineStepConfig.config`) —
   geeignet für generische/Dritt-Stages, die ihre eigene Konfiguration mitbringen.

Der Speicher-Fluss ist in beiden Fällen: UI ändert `AppSettings` → `requestAutoSave()`
→ FFI `ow_save_settings` → Festplatte → beim nächsten Lauf frisch geladen.

---

## 9. Referenz-Implementierung

Ein Konsument läuft heute durch den Vertrag — gute Vorlage:

- **LLM-Nachbearbeitungs-Stage** —
  `crates/torrowhisper-bridge/src/pipeline/stages/llm.rs`, `LlmStage::run`. Baut
  `BridgeHost::new("post_processing", …)` und ruft `host.generate(...)`.

(Das ausgebaute Chat-Plugin nutzte zusätzlich `host.chat(...)`/`chat_stream` im
Worker-Thread — bei Bedarf in der Historie unter `feat/chat-plugin` nachlesbar:
`crates/torrowhisper-bridge/src/chat.rs`.)

---

## 10. Minimal-Beispiel

Ein Plugin, das mit dem ersten lauffähigen Modell antwortet:

```rust
use std::sync::{Arc, atomic::AtomicBool};
use torrowhisper_bridge::plugin_api::{BridgeHost, PluginHost, LogLevel};

fn answer(settings: torrowhisper_core::AppSettings, frage: &str) -> Result<String, String> {
    let host = BridgeHost::new("beispiel", settings);

    // Erstes sofort nutzbares Modell wählen.
    let modell = host
        .ready_models()
        .into_iter()
        .next()
        .ok_or("Kein lauffähiges Sprachmodell verfügbar.")?;

    host.log(LogLevel::Info, &format!("nutze {}", modell.display_name));

    let cancelled = Arc::new(AtomicBool::new(false));
    host.chat(&modell.model_ref, "Du bist ein hilfreicher Assistent.", frage, &cancelled)
}
```

---

## 11. Dateien auf einen Blick

| Zweck | Datei |
|-------|-------|
| Host-Vertrag (`PluginHost`, `BridgeHost`) | `crates/torrowhisper-bridge/src/plugin_api.rs` |
| Modell-Registry-Bau | `crates/torrowhisper-bridge/src/llm/registry.rs` |
| Backend-Dispatch | `crates/torrowhisper-bridge/src/llm/mod.rs` (`provider_for`) |
| Modell-Typen / Verfügbarkeit | `crates/torrowhisper-core/src/lib.rs` (`LlmModelRef`, `LlmRegistryEntryDto`, `LlmAvailability`) |
| Logging-Fassade | `crates/torrowhisper-bridge/src/plugin_log.rs` |
| Pipeline-Stage-Verträge | `crates/torrowhisper-bridge/src/pipeline/mod.rs` |
| Referenz: LLM-Stage | `crates/torrowhisper-bridge/src/pipeline/stages/llm.rs` |
