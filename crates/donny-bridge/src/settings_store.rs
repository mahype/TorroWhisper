use std::{fs, io, path::PathBuf};

use directories::ProjectDirs;
use donny_core::AppSettings;

pub fn load() -> io::Result<AppSettings> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(invalid_data)
}

pub fn save(settings: &AppSettings) -> io::Result<PathBuf> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let bytes = serde_json::to_vec_pretty(settings).map_err(invalid_data)?;
    fs::write(&path, bytes)?;
    Ok(path)
}

fn config_path() -> io::Result<PathBuf> {
    ProjectDirs::from("com", "getdonny", "Donny")
        .map(|dirs| dirs.config_dir().join("settings.json"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "config directory unavailable"))
}

fn invalid_data(err: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
