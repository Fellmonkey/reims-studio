//! Reading and presenting the engine catalog shown in the launcher.

use std::path::Path;

use reims_core::contracts::{EngineIndex, EngineRelease, HostOs, Validate, load_json};

/// Loads engines for the current host from `index.json`, newest revision first.
///
/// # Errors
///
/// Returns the contract error when the index is missing or invalid.
pub fn load(path: &Path) -> Result<Vec<EngineRelease>, String> {
    let index: EngineIndex = load_json(path).map_err(|error| error.to_string())?;
    index.validate().map_err(|error| error.to_string())?;
    let host = HostOs::current();
    let mut engines: Vec<EngineRelease> = index
        .engines
        .into_iter()
        .filter(|engine| engine.os == host)
        .collect();
    engines.sort_by(|left, right| right.engine_id.cmp(&left.engine_id));
    Ok(engines)
}

/// Returns the archive size in a unit a person can read.
pub fn size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

/// Returns the compact one-line label used by the engine picker.
pub fn line(engine: &EngineRelease) -> String {
    format!(
        "{}  ·  {}  ·  {}  ·  macOS {}",
        engine.engine_id,
        engine.channel,
        size(engine.archive_size),
        engine.supported_macos_range
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_uses_binary_units() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(2048), "2 KiB");
        assert_eq!(size(3 * 1024 * 1024), "3 MiB");
    }
}
