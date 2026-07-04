//! Persistent store for chat sessions (#17 history sidebar).
//!
//! Mirrors `history_store`: the full list of [`ChatSession`]s is serialized to
//! `sessions.json` in the app config directory and rewritten on every change.

use std::{fs, io, path::PathBuf};

use directories::ProjectDirs;
use donnywhisper_core::ChatSession;

pub fn load() -> io::Result<Vec<ChatSession>> {
    let path = sessions_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(invalid_data)
}

pub fn save(sessions: &[ChatSession]) -> io::Result<PathBuf> {
    let path = sessions_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = serde_json::to_vec_pretty(sessions).map_err(invalid_data)?;
    fs::write(&path, bytes)?;
    Ok(path)
}

fn sessions_path() -> io::Result<PathBuf> {
    ProjectDirs::from("com", "getdonny", "DonnyWhisper")
        .map(|dirs| dirs.config_dir().join("sessions.json"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory unavailable"))
}

fn invalid_data(err: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
