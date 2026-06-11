//! Boundary translation for user-visible bridge strings.
//!
//! The bridge builds all status, summary, and diagnostic messages in English;
//! log output stays English too. This module translates those messages at the
//! FFI/DTO boundary, right before they are handed to the UI, based on the
//! configured UI language (`AppSettings::ui_language`).
//!
//! Two lookup layers:
//! 1. Exact match against the static table.
//! 2. Template match: an English template is split at its `{0}`/`{1}`/`{2}`
//!    placeholders, the literal segments are located in the message in order,
//!    and the extracted arguments are substituted into the German template.
//!    Arguments are translated recursively, so nested fragments such as the
//!    stop reason in "Recording stopped ({0})." are translated as well.
//!
//! Messages without a match (mostly `{err}` chains from the OS) pass through
//! unchanged.

use open_whisper_core::{AppSettings, UiLanguage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

pub fn lang(settings: &AppSettings) -> Lang {
    match settings.ui_language {
        UiLanguage::En => Lang::En,
        UiLanguage::De => Lang::De,
        UiLanguage::System => system_lang(),
    }
}

fn system_lang() -> Lang {
    match sys_locale::get_locale() {
        Some(locale) if locale.to_ascii_lowercase().starts_with("de") => Lang::De,
        _ => Lang::En,
    }
}

/// Translates a user-visible message. English (or unknown) input is returned
/// unchanged for `Lang::En`; for `Lang::De` the static table and the template
/// table are consulted, falling back to the original message.
pub fn translate(lang: Lang, message: &str) -> String {
    if lang == Lang::En {
        return message.to_owned();
    }
    translate_de(message, 0)
}

const MAX_RECURSION: usize = 3;

fn translate_de(message: &str, depth: usize) -> String {
    if let Some(translated) = static_de(message) {
        return translated.to_owned();
    }
    if depth < MAX_RECURSION
        && let Some(translated) = template_de(message, depth)
    {
        return translated;
    }
    message.to_owned()
}

/// Tries every template; the first whose literal segments appear in order
/// (anchored at start and end) wins. The near-anchorless nested templates are
/// only consulted while translating an extracted argument (depth > 0), so an
/// unknown top-level error containing " of " is not mangled.
fn template_de(message: &str, depth: usize) -> Option<String> {
    let tables: &[&[(&str, &str)]] = if depth > 0 {
        &[TEMPLATES_DE, TEMPLATES_DE_NESTED]
    } else {
        &[TEMPLATES_DE]
    };
    tables
        .iter()
        .find_map(|table| match_templates(table, message, depth))
}

fn match_templates(table: &[(&str, &str)], message: &str, depth: usize) -> Option<String> {
    'template: for (en, de) in table {
        let segments: Vec<&str> = split_template(en);
        debug_assert!(segments.len() >= 2, "template without placeholder: {en}");

        let mut args: Vec<&str> = Vec::with_capacity(segments.len() - 1);
        let mut rest = match message.strip_prefix(segments[0]) {
            Some(rest) => rest,
            None => continue,
        };
        for (index, segment) in segments.iter().enumerate().skip(1) {
            let is_last = index == segments.len() - 1;
            if is_last && segment.is_empty() {
                // Trailing placeholder: the remainder is the argument.
                args.push(rest);
                rest = "";
                break;
            }
            let found = if is_last {
                // Anchor the final literal at the end of the message.
                rest.len()
                    .checked_sub(segment.len())
                    .filter(|cut| rest.is_char_boundary(*cut) && &rest[*cut..] == *segment)
            } else {
                rest.find(segment)
            };
            let Some(cut) = found else { continue 'template };
            args.push(&rest[..cut]);
            rest = &rest[cut + segment.len()..];
        }
        if !rest.is_empty() {
            continue;
        }

        let mut result = de.to_string();
        for (index, arg) in args.iter().enumerate() {
            let translated = translate_de(arg, depth + 1);
            result = result.replace(&format!("{{{index}}}"), &translated);
        }
        return Some(result);
    }
    None
}

/// Splits "Download for {0} started." into ["Download for ", " started."].
fn split_template(template: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').expect("unclosed placeholder") + open;
        segments.push(&rest[..open]);
        rest = &rest[close + 1..];
    }
    segments.push(rest);
    segments
}

fn static_de(en: &str) -> Option<&'static str> {
    Some(match en {
        // Generic runtime status
        "Ready" => "Bereit",
        "History cleared." => "Historie geleert.",
        "Local transcription complete." => "Lokale Transkription abgeschlossen.",
        "Transcript ready." => "Transkript bereit.",
        "Transcript inserted into the active app." => "Transkript in die aktive App eingefügt.",
        "No text available to copy." => "Kein Text zum Kopieren vorhanden.",
        "No text available to paste." => "Kein Text zum Einfügen vorhanden.",
        "Dictation cancelled." => "Diktat abgebrochen.",
        "Dictation cancelled — saved to history." => {
            "Diktat abgebrochen — in Historie gespeichert."
        }
        "Insertion failed – text copied to clipboard." => {
            "Einfügen fehlgeschlagen – Text in Zwischenablage kopiert."
        }
        "Post-processing cancelled." => "Nachbearbeitung abgebrochen.",
        "Post-processing returned no text." => "Nachbearbeitung lieferte keinen Text.",
        "Post-processing worker stopped unexpectedly." => {
            "Nachbearbeitungsprozess wurde unerwartet beendet."
        }

        // Recording / transcription
        "Recording already in progress." => "Aufnahme läuft bereits.",
        "Recording was too short or empty." => "Aufnahme war zu kurz oder leer.",
        "Recording buffer could not be read." => "Aufnahmepuffer konnte nicht gelesen werden.",
        "No audio data available for Whisper." => "Keine Audiodaten für Whisper verfügbar.",
        "No default input device available." => "Kein Standard-Eingabegerät verfügbar.",
        "No input device found." => "Kein Eingabegerät gefunden.",
        "Transcription worker stopped unexpectedly." => {
            "Transkriptionsprozess wurde unerwartet beendet."
        }
        // Stop reasons, embedded in "Recording stopped ({0})."
        "key released" => "Taste losgelassen",
        "toggle stopped" => "manuell gestoppt",
        "silence detected" => "Stille erkannt",
        "menu bar action" => "Menüleisten-Aktion",
        "cancelled" => "abgebrochen",
        // Start suffixes, embedded in "Recording started via '{0}'{1}."
        ", silence stop active" => ", Silence-Stopp aktiv",
        ", manual stop active" => ", manuelles Stoppen aktiv",

        // Hotkey
        "Hotkey must not be empty." => "Der Hotkey darf nicht leer sein.",
        "Hotkey needs a real key like Space, R, or F8 in addition to modifier keys." => {
            "Der Hotkey braucht zusätzlich zu den Modifikatortasten eine echte Taste wie Leertaste, R oder F8."
        }

        // Launch at login
        "Startup status not synchronized yet." => "Systemstart noch nicht synchronisiert.",
        "Launch at login is active. 'Ask on first launch' leaves the OS state unchanged." => {
            "Start bei Anmeldung ist aktiv. ‚Beim ersten Start fragen' lässt die Systemeinstellung unverändert."
        }
        "Launch at login is inactive. 'Ask on first launch' leaves the OS state unchanged." => {
            "Start bei Anmeldung ist inaktiv. ‚Beim ersten Start fragen' lässt die Systemeinstellung unverändert."
        }
        "Launch at login is active and starts the app hidden." => {
            "Start bei Anmeldung ist aktiv; die App startet im Hintergrund."
        }
        "Launch at login should be active but could not be confirmed." => {
            "Start bei Anmeldung sollte aktiv sein, konnte aber nicht bestätigt werden."
        }
        "Launch at login should be disabled but is still active." => {
            "Start bei Anmeldung sollte deaktiviert sein, ist aber noch aktiv."
        }
        "Launch at login is disabled." => "Start bei Anmeldung ist deaktiviert.",
        "Launch at login enabled." => "Start bei Anmeldung aktiviert.",
        "Launch at login disabled." => "Start bei Anmeldung deaktiviert.",
        "Launch at login could not be confirmed." => {
            "Start bei Anmeldung konnte nicht bestätigt werden."
        }
        "Launch at login is still active." => "Start bei Anmeldung ist noch aktiv.",

        // Model summaries / downloads
        "No model status loaded yet." => "Noch kein Modellstatus geladen.",
        "Local model ready." => "Lokales Modell bereit.",
        "Local model has not been downloaded yet." => {
            "Lokales Modell wurde noch nicht heruntergeladen."
        }
        "Local model path is not currently resolvable." => {
            "Lokaler Modellpfad ist derzeit nicht auflösbar."
        }
        "Local language model ready." => "Lokales Sprachmodell bereit.",
        "Local language model has not been downloaded yet." => {
            "Lokales Sprachmodell wurde noch nicht heruntergeladen."
        }
        "Local language model path is not currently resolvable." => {
            "Pfad des lokalen Sprachmodells ist derzeit nicht auflösbar."
        }
        "A model download is already running." => "Ein Modell-Download läuft bereits.",
        "A language model download is already running." => {
            "Ein Sprachmodell-Download läuft bereits."
        }
        "A running download can't be deleted at the same time." => {
            "Ein laufender Download kann nicht gleichzeitig gelöscht werden."
        }
        "Download worker stopped unexpectedly." => "Download-Prozess wurde unerwartet beendet.",
        "Language model download worker stopped unexpectedly." => {
            "Sprachmodell-Download-Prozess wurde unerwartet beendet."
        }
        "Config directory for models not available." => {
            "Konfigurationsverzeichnis für Modelle nicht verfügbar."
        }
        "Config directory for language models not available." => {
            "Konfigurationsverzeichnis für Sprachmodelle nicht verfügbar."
        }
        "Custom language model has no ID." => "Eigenes Sprachmodell hat keine ID.",
        "URL for custom language model is empty." => "URL für eigenes Sprachmodell ist leer.",

        // Post-processing providers
        "Ollama response contained no processed text." => {
            "Ollama-Antwort enthielt keinen verarbeiteten Text."
        }
        "LM Studio response contained no processed text." => {
            "LM-Studio-Antwort enthielt keinen verarbeiteten Text."
        }

        // Diagnostics
        "Diagnostics loading." => "Diagnose wird geladen.",
        "Diagnostics: no open issues detected." => "Diagnose: keine offenen Probleme gefunden.",
        "Microphone" => "Mikrofon",
        "Input device" => "Eingabegerät",
        "Local model" => "Lokales Modell",
        "Global hotkey" => "Globaler Hotkey",
        "macOS privacy" => "macOS-Datenschutz",
        "No action needed." => "Keine Aktion nötig.",
        "No input device was detected." => "Es wurde kein Eingabegerät erkannt.",
        "Check microphone permissions and that at least one input device is connected." => {
            "Prüfe die Mikrofon-Berechtigungen und ob mindestens ein Eingabegerät angeschlossen ist."
        }
        "Pick a different microphone in Onboarding or Settings." => {
            "Wähle im Onboarding oder in den Einstellungen ein anderes Mikrofon."
        }
        "Local dictation is ready to use." => "Lokales Diktieren ist einsatzbereit.",
        "Download the selected model before your first dictation." => {
            "Lade das ausgewählte Modell vor deinem ersten Diktat herunter."
        }
        "Check the model path or pick one of the built-in presets again." => {
            "Prüfe den Modellpfad oder wähle erneut eines der integrierten Presets."
        }
        "Check the hotkey combination and, on macOS, grant Accessibility or Input Monitoring permission if needed." => {
            "Prüfe die Hotkey-Kombination und erteile unter macOS bei Bedarf die Berechtigung für Bedienungshilfen oder Eingabeüberwachung."
        }
        "The hotkey integration could not be initialized." => {
            "Die Hotkey-Integration konnte nicht initialisiert werden."
        }
        "Restart the app and check whether the combination is already in use by another app." => {
            "Starte die App neu und prüfe, ob die Kombination bereits von einer anderen App verwendet wird."
        }
        "Autostart" => "Autostart",
        "Adjust the behavior in Onboarding or Settings." => {
            "Passe das Verhalten im Onboarding oder in den Einstellungen an."
        }
        "macOS may require privacy permissions for the microphone, global hotkey, and pasting into other apps." => {
            "macOS verlangt ggf. Datenschutz-Berechtigungen für Mikrofon, globalen Hotkey und das Einfügen in andere Apps."
        }
        "If you run into issues, open System Settings > Privacy & Security and check Microphone, Accessibility, and Input Monitoring." => {
            "Öffne bei Problemen Systemeinstellungen > Datenschutz & Sicherheit und prüfe Mikrofon, Bedienungshilfen und Eingabeüberwachung."
        }
        _ => return None,
    })
}

/// English template → German template. Literal segments must be unambiguous
/// enough to anchor the match; every template is anchored at the start and
/// the end of the message. Order matters: more specific templates first.
const TEMPLATES_DE: &[(&str, &str)] = &[
    // Recording lifecycle
    (
        "Recording started via '{0}'{1}.",
        "Aufnahme gestartet über '{0}'{1}.",
    ),
    (
        "Recording stopped ({0}). Local transcription in progress.",
        "Aufnahme gestoppt ({0}). Lokale Transkription läuft.",
    ),
    (
        "Recording blocked: {0} has not been downloaded yet.",
        "Aufnahme blockiert: {0} wurde noch nicht heruntergeladen.",
    ),
    (
        "Whisper transcript ready. Post-processing '{0}' running.",
        "Whisper-Transkript bereit. Nachbearbeitung '{0}' läuft.",
    ),
    (
        "Whisper recognized no text. Model: {0}, language: {1}.",
        "Whisper hat keinen Text erkannt. Modell: {0}, Sprache: {1}.",
    ),
    // Microphone switching
    ("Microphone active: {0}.", "Mikrofon aktiv: {0}."),
    (
        "Microphone '{0}' unavailable — using '{1}'.",
        "Mikrofon '{0}' nicht verfügbar — verwende '{1}'.",
    ),
    (
        "Mic switch failed: {0}. Recording stopped — please restart.",
        "Mikrofonwechsel fehlgeschlagen: {0}. Aufnahme gestoppt — bitte neu starten.",
    ),
    // Downloads (model + language model share the phrasing)
    ("Download for {0} started.", "Download für {0} gestartet."),
    (
        "Download complete: {0} ({1})",
        "Download abgeschlossen: {0} ({1})",
    ),
    (
        "Download for {0} has been running for {1} ({2}).",
        "Download für {0} läuft seit {1} ({2}).",
    ),
    (
        "Last model download failed: {0}",
        "Letzter Modell-Download fehlgeschlagen: {0}",
    ),
    (
        "Last language model download failed: {0}",
        "Letzter Sprachmodell-Download fehlgeschlagen: {0}",
    ),
    (
        "Language model loaded: {0} ({1})",
        "Sprachmodell geladen: {0} ({1})",
    ),
    ("Local model ready ({0})", "Lokales Modell bereit ({0})"),
    (
        "{0} has not been downloaded yet.",
        "{0} wurde noch nicht heruntergeladen.",
    ),
    ("Download for {0} in progress.", "Download für {0} läuft."),
    ("{0} ({1}) not loaded yet.", "{0} ({1}) noch nicht geladen."),
    ("{0} not loaded yet.", "{0} noch nicht geladen."),
    ("{0} ready.", "{0} bereit."),
    ("Local file: {0}", "Lokale Datei: {0}"),
    ("Download URL: {0}", "Download-URL: {0}"),
    (
        "Custom language model '{0}' not found.",
        "Eigenes Sprachmodell '{0}' nicht gefunden.",
    ),
    (
        "'{0}' is a locally selected model — no download needed.",
        "'{0}' ist ein lokal ausgewähltes Modell — kein Download nötig.",
    ),
    (
        "Removed old language models ({0} file(s)). Gemma 4 is now used.",
        "Alte Sprachmodelle entfernt ({0} Datei(en)). Es wird jetzt Gemma 4 verwendet.",
    ),
    // Diagnostics
    (
        "{0} input device(s) detected.",
        "{0} Eingabegerät(e) erkannt.",
    ),
    (
        "The selected device '{0}' is available.",
        "Das ausgewählte Gerät '{0}' ist verfügbar.",
    ),
    (
        "The selected device '{0}' is not currently available.",
        "Das ausgewählte Gerät '{0}' ist derzeit nicht verfügbar.",
    ),
    (
        "Hotkey '{0}' is registered.",
        "Hotkey '{0}' ist registriert.",
    ),
    (
        "Hotkey '{0}' is not active yet.",
        "Hotkey '{0}' ist noch nicht aktiv.",
    ),
    (
        "Diagnostics: {0} warning(s), no errors.",
        "Diagnose: {0} Warnung(en), keine Fehler.",
    ),
    (
        "Diagnostics: {0} error(s), {1} warning(s).",
        "Diagnose: {0} Fehler, {1} Warnung(en).",
    ),
    // Provider summary
    ("Local Whisper with {0}", "Lokales Whisper mit {0}"),
    ("Local Whisper + {0} ({1})", "Lokales Whisper + {0} ({1})"),
    // History
    (
        "History entry {0} deleted.",
        "Historien-Eintrag {0} gelöscht.",
    ),
    (
        "History entry {0} not found.",
        "Historien-Eintrag {0} nicht gefunden.",
    ),
    // Common error chains (the {err} tail stays as the OS reports it)
    (
        "Settings could not be saved: {0}",
        "Einstellungen konnten nicht gespeichert werden: {0}",
    ),
    (
        "Settings could not be loaded: {0}",
        "Einstellungen konnten nicht geladen werden: {0}",
    ),
    (
        "History could not be saved: {0}",
        "Historie konnte nicht gespeichert werden: {0}",
    ),
    (
        "Model download failed: {0}",
        "Modell-Download fehlgeschlagen: {0}",
    ),
    (
        "Model could not be deleted: {0}",
        "Modell konnte nicht gelöscht werden: {0}",
    ),
    (
        "Language model could not be deleted: {0}",
        "Sprachmodell konnte nicht gelöscht werden: {0}",
    ),
    (
        "Whisper model could not be loaded: {0}",
        "Whisper-Modell konnte nicht geladen werden: {0}",
    ),
    (
        "Whisper transcription failed: {0}",
        "Whisper-Transkription fehlgeschlagen: {0}",
    ),
    (
        "Audio recording could not be started: {0}",
        "Audioaufnahme konnte nicht gestartet werden: {0}",
    ),
    (
        "Audio recording could not be restarted: {0}",
        "Audioaufnahme konnte nicht neu gestartet werden: {0}",
    ),
    ("Audio error in stream: {0}", "Audiofehler im Stream: {0}"),
    (
        "Input device '{0}' was not found.",
        "Eingabegerät '{0}' wurde nicht gefunden.",
    ),
    (
        "Input devices could not be loaded: {0}",
        "Eingabegeräte konnten nicht geladen werden: {0}",
    ),
    (
        "Startup status could not be read: {0}",
        "Systemstart-Status konnte nicht gelesen werden: {0}",
    ),
    (
        "Startup not available: {0}",
        "Systemstart nicht verfügbar: {0}",
    ),
    (
        "Launch at login could not be enabled: {0}",
        "Start bei Anmeldung konnte nicht aktiviert werden: {0}",
    ),
    (
        "Launch at login could not be disabled: {0}",
        "Start bei Anmeldung konnte nicht deaktiviert werden: {0}",
    ),
    (
        "Launch at login could not be confirmed: {0}",
        "Start bei Anmeldung konnte nicht bestätigt werden: {0}",
    ),
    (
        "{0} Clipboard fallback: {1}",
        "{0} Zwischenablage-Fallback: {1}",
    ),
];

/// Templates with too little literal anchoring to be safe at the top level.
/// Used only for arguments extracted by an outer template (download progress
/// such as "1.2 GB of 4.1 GB").
const TEMPLATES_DE_NESTED: &[(&str, &str)] = &[
    ("{0} of {1}", "{0} von {1}"),
    ("{0} downloaded", "{0} heruntergeladen"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_passes_through() {
        assert_eq!(
            translate(Lang::En, "Transcript ready."),
            "Transcript ready."
        );
    }

    #[test]
    fn static_lookup_translates() {
        assert_eq!(
            translate(Lang::De, "Transcript ready."),
            "Transkript bereit."
        );
    }

    #[test]
    fn unknown_message_passes_through() {
        assert_eq!(translate(Lang::De, "Some odd error."), "Some odd error.");
    }

    #[test]
    fn template_extracts_and_translates_args() {
        assert_eq!(
            translate(Lang::De, "Download for Whisper Small started."),
            "Download für Whisper Small gestartet."
        );
        assert_eq!(
            translate(Lang::De, "Hotkey 'Ctrl+Shift+Space' is registered."),
            "Hotkey 'Ctrl+Shift+Space' ist registriert."
        );
    }

    #[test]
    fn nested_arguments_are_translated() {
        assert_eq!(
            translate(
                Lang::De,
                "Recording stopped (silence detected). Local transcription in progress."
            ),
            "Aufnahme gestoppt (Stille erkannt). Lokale Transkription läuft."
        );
        assert_eq!(
            translate(
                Lang::De,
                "Download for Gemma 4 E4B has been running for 2m 10s (1.2 GB of 4.1 GB)."
            ),
            "Download für Gemma 4 E4B läuft seit 2m 10s (1.2 GB von 4.1 GB)."
        );
    }

    #[test]
    fn start_suffix_is_translated() {
        assert_eq!(
            translate(
                Lang::De,
                "Recording started via 'MacBook Pro Microphone', silence stop active."
            ),
            "Aufnahme gestartet über 'MacBook Pro Microphone', Silence-Stopp aktiv."
        );
    }

    #[test]
    fn trailing_placeholder_captures_remainder() {
        assert_eq!(
            translate(Lang::De, "Model download failed: connection reset"),
            "Modell-Download fehlgeschlagen: connection reset"
        );
    }
}
