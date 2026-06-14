//! Central logging facade for plugins (#15).
//!
//! Every plugin — the built-in chat plugin now, third-party plugins later —
//! routes its log lines through here instead of calling `log::` directly. That
//! gives all plugin output a consistent `plugin:<id>` target in the one shared
//! app log, so afterwards you can trace exactly what a plugin did or why it
//! failed. This is the logging half of the shared plugin host API.

/// Logs an informational line on behalf of `plugin_id`.
pub fn info(plugin_id: &str, message: &str) {
    log::info!(target: &target(plugin_id), "{message}");
}

/// Logs a warning on behalf of `plugin_id`.
pub fn warn(plugin_id: &str, message: &str) {
    log::warn!(target: &target(plugin_id), "{message}");
}

/// Logs an error on behalf of `plugin_id`.
pub fn error(plugin_id: &str, message: &str) {
    log::error!(target: &target(plugin_id), "{message}");
}

/// Logs a debug line on behalf of `plugin_id`.
pub fn debug(plugin_id: &str, message: &str) {
    log::debug!(target: &target(plugin_id), "{message}");
}

fn target(plugin_id: &str) -> String {
    format!("plugin:{plugin_id}")
}
