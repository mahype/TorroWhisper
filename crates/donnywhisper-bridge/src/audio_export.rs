//! Optional on-disk export of dictations: the recorded audio as MP3 and/or the
//! transcript as a plain-text file. Used when the user enables saving in
//! Settings and picks a destination folder.

use std::path::{Path, PathBuf};

use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
use donnywhisper_core::AppSettings;

/// Builds the timestamped base name (without extension) shared by a dictation's
/// audio and transcript files, e.g. `dictation-1781020800`.
pub fn base_name(unix_secs: i64) -> String {
    format!("dictation-{unix_secs}")
}

/// Whether any on-disk saving is configured (a destination plus at least one of
/// the audio/transcript toggles).
pub fn saving_enabled(settings: &AppSettings) -> bool {
    !settings.save_directory.trim().is_empty()
        && (settings.save_audio_recordings || settings.save_transcripts)
}

fn destination(settings: &AppSettings, base: &str, ext: &str) -> PathBuf {
    PathBuf::from(&settings.save_directory).join(format!("{base}.{ext}"))
}

/// MP3 destination for this dictation, or `None` if audio saving is off / unset.
pub fn audio_destination(settings: &AppSettings, base: &str) -> Option<PathBuf> {
    guard_directory(settings)?;
    if settings.save_audio_recordings {
        Some(destination(settings, base, "mp3"))
    } else {
        None
    }
}

fn guard_directory(settings: &AppSettings) -> Option<()> {
    if settings.save_directory.trim().is_empty() {
        None
    } else {
        Some(())
    }
}

/// Encodes mono f32 PCM to a 128 kbps MP3 and writes it to `path`.
pub fn write_mp3(samples: &[f32], sample_rate: u32, path: &Path) -> Result<(), String> {
    let mut builder = Builder::new().ok_or("could not create MP3 encoder")?;
    builder
        .set_num_channels(1)
        .map_err(|err| format!("MP3 channels: {err:?}"))?;
    builder
        .set_sample_rate(sample_rate)
        .map_err(|err| format!("MP3 sample rate: {err:?}"))?;
    builder
        .set_brate(Bitrate::Kbps128)
        .map_err(|err| format!("MP3 bitrate: {err:?}"))?;
    builder
        .set_quality(Quality::Good)
        .map_err(|err| format!("MP3 quality: {err:?}"))?;
    let mut encoder = builder
        .build()
        .map_err(|err| format!("MP3 encoder init: {err:?}"))?;

    let mut out: Vec<u8> =
        Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(samples.len()));
    encoder
        .encode_to_vec(MonoPcm(samples), &mut out)
        .map_err(|err| format!("MP3 encode: {err:?}"))?;
    encoder
        .flush_to_vec::<FlushNoGap>(&mut out)
        .map_err(|err| format!("MP3 flush: {err:?}"))?;

    write_bytes(path, &out)
}

/// Writes the transcript to `{save_directory}/{base}.txt`. No-op if transcript
/// saving is off or no directory is set.
pub fn write_transcript(
    settings: &AppSettings,
    base: &str,
    transcript: &str,
) -> Result<(), String> {
    if guard_directory(settings).is_none() || !settings.save_transcripts {
        return Ok(());
    }
    let path = destination(settings, base, "txt");
    write_bytes(&path, transcript.as_bytes())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create folder: {err}"))?;
    }
    std::fs::write(path, bytes).map_err(|err| format!("write {}: {err}", path.display()))
}
