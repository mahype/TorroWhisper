use donnywhisper_core::{AppSettings, DiagnosticItemDto, DiagnosticStatus, DiagnosticsDto};

use crate::{dictation::DictationController, hotkey::HotKeyController, model_manager};

pub fn collect(
    settings: &AppSettings,
    dictation: &DictationController,
    hotkey: Option<&HotKeyController>,
    autostart_summary: &str,
) -> DiagnosticsDto {
    let mut items = Vec::new();
    let available_devices = dictation.available_input_devices();

    if available_devices.is_empty() {
        items.push(item(
            "Microphone",
            DiagnosticStatus::Error,
            "No input device was detected.",
            "Check microphone permissions and that at least one input device is connected.",
        ));
    } else {
        items.push(item(
            "Microphone",
            DiagnosticStatus::Ok,
            &format!("{} input device(s) detected.", available_devices.len()),
            "No action needed.",
        ));
    }

    let selected_device_ok = settings.input_device_name == "System Default"
        || available_devices
            .iter()
            .any(|device| device == &settings.input_device_name);
    if selected_device_ok {
        items.push(item(
            "Input device",
            DiagnosticStatus::Ok,
            &format!(
                "The selected device '{}' is available.",
                settings.input_device_name
            ),
            "No action needed.",
        ));
    } else {
        items.push(item(
            "Input device",
            DiagnosticStatus::Warning,
            &format!(
                "The selected device '{}' is not currently available.",
                settings.input_device_name
            ),
            "Pick a different microphone in Onboarding or Settings.",
        ));
    }

    match model_manager::resolve_model_path(settings) {
        Ok(path) if path.exists() => items.push(item(
            "Local model",
            DiagnosticStatus::Ok,
            &model_ready_message(settings),
            "Local dictation is ready to use.",
        )),
        Ok(_) => items.push(item(
            "Local model",
            DiagnosticStatus::Warning,
            &format!(
                "{} has not been downloaded yet.",
                settings.local_model.display_label()
            ),
            "Download the selected model before your first dictation.",
        )),
        Err(err) => items.push(item(
            "Local model",
            DiagnosticStatus::Error,
            &err,
            "Check the model path or pick one of the built-in presets again.",
        )),
    }

    if let Some(hotkey) = hotkey {
        if hotkey.is_registered() {
            items.push(item(
                "Global hotkey",
                DiagnosticStatus::Ok,
                &format!("Hotkey '{}' is registered.", settings.hotkey),
                "No action needed.",
            ));
        } else {
            items.push(item(
                "Global hotkey",
                DiagnosticStatus::Warning,
                &format!(
                    "Hotkey '{}' is not active yet.",
                    settings.hotkey
                ),
                "Check the hotkey combination and, on macOS, grant Accessibility or Input Monitoring permission if needed.",
            ));
        }
    } else {
        items.push(item(
            "Global hotkey",
            DiagnosticStatus::Error,
            "The hotkey integration could not be initialized.",
            "Restart the app and check whether the combination is already in use by another app.",
        ));
    }

    items.push(item(
        "Autostart",
        DiagnosticStatus::Info,
        autostart_summary,
        "Adjust the behavior in Onboarding or Settings.",
    ));

    #[cfg(target_os = "macos")]
    {
        items.push(item(
            "macOS privacy",
            DiagnosticStatus::Info,
            "macOS may require privacy permissions for the microphone, global hotkey, and pasting into other apps.",
            "If you run into issues, open System Settings > Privacy & Security and check Microphone, Accessibility, and Input Monitoring.",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        items.push(item(
            "Linux session",
            DiagnosticStatus::Info,
            "Global hotkeys and simulated paste are usually more robust under X11 than under Wayland.",
            "If the hotkey or paste does not work, try an X11 session or desktop-specific permissions.",
        ));
    }

    #[cfg(target_os = "windows")]
    {
        items.push(item(
            "Windows focus",
            DiagnosticStatus::Info,
            "Some apps block simulated input while UAC dialogs or security software are active.",
            "Test pasting in a plain text app and check focus or security software if needed.",
        ));
    }

    let error_count = items
        .iter()
        .filter(|item| item.status == DiagnosticStatus::Error)
        .count();
    let warning_count = items
        .iter()
        .filter(|item| item.status == DiagnosticStatus::Warning)
        .count();

    let summary = match (error_count, warning_count) {
        (0, 0) => "Diagnostics: no open issues detected.".to_owned(),
        (0, warnings) => format!("Diagnostics: {warnings} warning(s), no errors."),
        (errors, warnings) => format!("Diagnostics: {errors} error(s), {warnings} warning(s)."),
    };

    DiagnosticsDto { summary, items }
}

fn item(
    title: &str,
    status: DiagnosticStatus,
    problem: &str,
    recommendation: &str,
) -> DiagnosticItemDto {
    DiagnosticItemDto {
        title: title.to_owned(),
        status,
        problem: problem.to_owned(),
        recommendation: recommendation.to_owned(),
    }
}

fn model_ready_message(settings: &AppSettings) -> String {
    format!(
        "{} is available locally.",
        settings.local_model.display_label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use donnywhisper_core::AppSettings;

    #[test]
    fn diagnostics_summary_reports_errors_and_warnings() {
        let settings = AppSettings::default();
        let dictation = DictationController::new();
        let report = collect(&settings, &dictation, None, "Autostart unknown");

        assert!(report.summary.contains("error"));
    }
}
