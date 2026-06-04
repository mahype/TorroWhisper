use std::{fs, io, path::PathBuf};

use directories::ProjectDirs;
use open_whisper_core::HistoryEntry;

pub fn load() -> io::Result<Vec<HistoryEntry>> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(invalid_data)
}

pub fn save(entries: &[HistoryEntry]) -> io::Result<PathBuf> {
    let path = history_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = serde_json::to_vec_pretty(entries).map_err(invalid_data)?;
    fs::write(&path, bytes)?;
    Ok(path)
}

pub fn append(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry, cap: usize) {
    entries.insert(0, entry);
    if cap > 0 && entries.len() > cap {
        entries.truncate(cap);
    }
}

pub fn delete(entries: &mut Vec<HistoryEntry>, id: &str) -> bool {
    let original_len = entries.len();
    entries.retain(|entry| entry.id != id);
    entries.len() != original_len
}

pub fn clear(entries: &mut Vec<HistoryEntry>) {
    entries.clear();
}

fn history_path() -> io::Result<PathBuf> {
    ProjectDirs::from("dev", "awesome", "open-whisper")
        .map(|dirs| dirs.config_dir().join("history.json"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory unavailable"))
}

fn invalid_data(err: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
