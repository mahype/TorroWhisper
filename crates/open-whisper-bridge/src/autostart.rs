use std::{env, path::PathBuf};

use auto_launch::AutoLaunchBuilder;
use open_whisper_core::{AppSettings, StartupBehavior};

pub const START_HIDDEN_FLAG: &str = "--hidden";

pub struct AutostartManager {
    launcher: Option<auto_launch::AutoLaunch>,
    status: String,
    enabled: Option<bool>,
}

impl AutostartManager {
    pub fn new() -> Self {
        match build_launcher() {
            Ok(launcher) => {
                let enabled = launcher.is_enabled().ok();
                Self {
                    launcher: Some(launcher),
                    status: "Startup status not synchronized yet.".to_owned(),
                    enabled,
                }
            }
            Err(err) => Self {
                launcher: None,
                status: format!("Startup not available: {err}"),
                enabled: None,
            },
        }
    }

    pub fn sync_saved_settings(
        &mut self,
        settings: &AppSettings,
    ) -> Result<Option<String>, String> {
        let Some(launcher) = &self.launcher else {
            return Err(self.status.clone());
        };

        match settings.startup_behavior {
            StartupBehavior::AskOnFirstLaunch => {
                let enabled = launcher
                    .is_enabled()
                    .map_err(|err| format!("Startup status could not be read: {err}"))?;
                self.enabled = Some(enabled);
                self.status = if enabled {
                    "Launch at login is active. 'Ask on first launch' leaves the OS state unchanged.".to_owned()
                } else {
                    "Launch at login is inactive. 'Ask on first launch' leaves the OS state unchanged.".to_owned()
                };
                Ok(None)
            }
            StartupBehavior::LaunchAtLogin => {
                let was_enabled = launcher
                    .is_enabled()
                    .map_err(|err| format!("Startup status could not be read: {err}"))?;
                let stale = was_enabled && has_stale_program_path();

                if stale {
                    let _ = launcher.disable();
                }

                if !was_enabled || stale {
                    launcher
                        .enable()
                        .map_err(|err| format!("Launch at login could not be enabled: {err}"))?;
                }

                let enabled = launcher
                    .is_enabled()
                    .map_err(|err| format!("Launch at login could not be confirmed: {err}"))?;
                self.enabled = Some(enabled);
                self.status = if enabled {
                    "Launch at login is active and starts the app hidden.".to_owned()
                } else {
                    "Launch at login should be active but could not be confirmed.".to_owned()
                };
                Ok(Some(if enabled {
                    "Launch at login enabled.".to_owned()
                } else {
                    "Launch at login could not be confirmed.".to_owned()
                }))
            }
            StartupBehavior::ManualLaunch => {
                if launcher
                    .is_enabled()
                    .map_err(|err| format!("Startup status could not be read: {err}"))?
                {
                    launcher
                        .disable()
                        .map_err(|err| format!("Launch at login could not be disabled: {err}"))?;
                }

                let enabled = launcher
                    .is_enabled()
                    .map_err(|err| format!("Launch at login could not be confirmed: {err}"))?;
                self.enabled = Some(enabled);
                self.status = if enabled {
                    "Launch at login should be disabled but is still active.".to_owned()
                } else {
                    "Launch at login is disabled.".to_owned()
                };
                Ok(Some(if enabled {
                    "Launch at login is still active.".to_owned()
                } else {
                    "Launch at login disabled.".to_owned()
                }))
            }
        }
    }

    pub fn summary(&self) -> &str {
        &self.status
    }
    pub fn start_hidden_requested() -> bool {
        env::args().any(|arg| arg == START_HIDDEN_FLAG)
    }
}

fn build_launcher() -> Result<auto_launch::AutoLaunch, String> {
    let executable_path = current_executable_path()?;
    let app_path = executable_path
        .to_str()
        .ok_or_else(|| "Path to current app is not UTF-8.".to_owned())?;

    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name("open-whisper")
        .set_app_path(app_path)
        .set_args(&[START_HIDDEN_FLAG]);

    #[cfg(target_os = "linux")]
    {
        use auto_launch::LinuxLaunchMode;
        builder.set_linux_launch_mode(LinuxLaunchMode::XdgAutostart);
    }

    #[cfg(target_os = "macos")]
    {
        use auto_launch::MacOSLaunchMode;
        builder.set_macos_launch_mode(MacOSLaunchMode::LaunchAgent);
    }

    #[cfg(target_os = "windows")]
    {
        use auto_launch::WindowsEnableMode;
        builder.set_windows_enable_mode(WindowsEnableMode::CurrentUser);
    }

    builder.build().map_err(|err| err.to_string())
}

fn current_executable_path() -> Result<PathBuf, String> {
    let path = env::current_exe().map_err(|err| format!("Path to app not available: {err}"))?;
    if !path.is_absolute() {
        return Err("Path to app is not absolute.".to_owned());
    }

    Ok(path)
}

#[cfg(target_os = "macos")]
fn has_stale_program_path() -> bool {
    let Some(registered) = read_registered_program_path() else {
        return false;
    };
    let Ok(current) = env::current_exe() else {
        return false;
    };
    registered != current
}

#[cfg(not(target_os = "macos"))]
fn has_stale_program_path() -> bool {
    false
}

#[cfg(target_os = "macos")]
fn read_registered_program_path() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    let plist = PathBuf::from(home).join("Library/LaunchAgents/open-whisper.plist");
    let content = std::fs::read_to_string(&plist).ok()?;
    content
        .split("<key>ProgramArguments</key>")
        .nth(1)?
        .split("<string>")
        .nth(1)?
        .split("</string>")
        .next()
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_flag_constant_matches_cli_expectation() {
        assert_eq!(START_HIDDEN_FLAG, "--hidden");
    }
}
