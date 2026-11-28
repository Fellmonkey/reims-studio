//! Reading and presenting the engine catalog shown in the launcher.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use reims_core::contracts::{EngineIndex, EngineRelease, HostOs, Validate, load_json, parse_json};
use reims_engine_mgr::{engine_dir, unpack_tar_zst, verify_archive};

/// URL of the published release index used by Refresh.
const INDEX_URL: &str =
    "https://github.com/Fellmonkey/reims-studio/releases/download/engines-master/index.json";
/// Refuse absurdly large indexes before parsing them.
const MAX_INDEX_BYTES: usize = 1024 * 1024;

/// Where a catalog came from, shown in the UI so results are not ambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    /// Downloaded from the GitHub release.
    Remote,
}

/// Loads engines for the current host from a local `index.json`, newest first.
///
/// # Errors
///
/// Returns the contract error when the index is missing or invalid.
pub fn load(path: &Path) -> Result<Vec<EngineRelease>, String> {
    let index: EngineIndex = load_json(path).map_err(|error| error.to_string())?;
    host_engines(index)
}

/// Downloads the release index, caches it locally and returns host engines.
///
/// # Errors
///
/// Returns a network, size or contract error; callers fall back to [`load`].
pub fn fetch(root: &Path) -> Result<(Vec<EngineRelease>, CatalogSource), String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .https_only(true)
        .build();
    let agent: ureq::Agent = config.into();
    let raw = agent
        .get(INDEX_URL)
        .call()
        .map_err(|error| format!("download failed: {error}"))?
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("read failed: {error}"))?;
    if raw.len() > MAX_INDEX_BYTES {
        return Err(String::from("release index is too large"));
    }
    let index: EngineIndex =
        parse_json(&raw, Path::new("release/index.json")).map_err(|error| error.to_string())?;
    cache(root, raw.as_bytes())?;
    let engines = host_engines(index)?;
    Ok((engines, CatalogSource::Remote))
}

/// Writes the downloaded index to the portable folder atomically.
fn cache(root: &Path, raw: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let final_path = root.join("index.json");
    let temporary = root.join("index.json.tmp");
    std::fs::write(&temporary, raw).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &final_path).map_err(|error| error.to_string())
}

/// Downloads, verifies and unpacks an engine archive into the portable folder.
///
/// `downloaded` is updated as bytes arrive so the UI can show real progress.
///
/// # Errors
///
/// Returns a network, hash or archive error and leaves the partial download
/// in a `.part` file so a retry does not claim the archive is installed.
pub fn install(
    root: &Path,
    engine: &EngineRelease,
    downloaded: Arc<AtomicU64>,
) -> Result<(), String> {
    let cache = root.join("cache");
    std::fs::create_dir_all(&cache).map_err(|error| error.to_string())?;
    let partial = cache.join(format!("{}.tar.zst.part", engine.engine_id));
    let archive = cache.join(format!("{}.tar.zst", engine.engine_id));

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .https_only(true)
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(&engine.url)
        .call()
        .map_err(|error| format!("download failed: {error}"))?;
    let mut file = File::create(&partial).map_err(|error| error.to_string())?;
    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0_u8; 64 * 1024];
    downloaded.store(0, Ordering::Relaxed);
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("download read failed: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        downloaded.fetch_add(read as u64, Ordering::Relaxed);
    }
    drop(file);

    verify_archive(&partial, &engine.archive_sha256).map_err(|error| error.to_string())?;
    std::fs::rename(&partial, &archive).map_err(|error| error.to_string())?;
    let destination = engine_dir(root, &engine.engine_id);
    unpack_tar_zst(&archive, &destination).map_err(|error| error.to_string())?;
    Ok(())
}

/// Filters an index to the current host and sorts newest revision first.
fn host_engines(index: EngineIndex) -> Result<Vec<EngineRelease>, String> {
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

/// One commit entry parsed from the generated changelog Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEntry {
    /// Short commit SHA.
    pub sha: String,
    /// Author date as published by GitHub.
    pub date: String,
    /// First line of the commit message.
    pub subject: String,
    /// Remaining commit message lines.
    pub body: String,
}

/// Parses `## <sha> · <date>` sections into accordion entries.
pub fn parse_changelog(raw: &str) -> Vec<CommitEntry> {
    let mut entries = Vec::new();
    let mut current: Option<CommitEntry> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            let (sha, date) = rest.split_once(" · ").unwrap_or((rest, ""));
            current = Some(CommitEntry {
                sha: sha.trim().to_string(),
                date: date.trim().to_string(),
                subject: String::new(),
                body: String::new(),
            });
        } else if let Some(entry) = current.as_mut() {
            if let Some(subject) = line.strip_prefix("### ") {
                entry.subject = subject.trim().to_string();
            } else if entry.subject.is_empty() && line.trim().is_empty() {
                continue;
            } else {
                if !entry.body.is_empty() {
                    entry.body.push('\n');
                }
                entry.body.push_str(line);
            }
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    entries.retain(|entry| !entry.sha.is_empty() && !entry.sha.starts_with('#'));
    entries
}

/// Returns the release page for an engine's download URL.
pub fn release_url(engine: &EngineRelease) -> Option<String> {
    let marker = "/releases/download/";
    let (prefix, rest) = engine.url.split_once(marker)?;
    let tag = rest.split('/').next()?;
    if tag.is_empty() {
        return None;
    }
    Some(format!("{prefix}/releases/tag/{tag}"))
}

/// Returns true when an engine build advertises support for a macOS version.
pub fn supports_macos(engine: &EngineRelease, version: &str) -> bool {
    let range = engine.supported_macos_range.trim();
    if let Some(minimum) = range.strip_suffix('+') {
        let parse = |value: &str| -> Option<(u32, u32)> {
            let mut parts = value.trim().split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next().unwrap_or("0").parse().ok()?;
            Some((major, minor))
        };
        match (parse(minimum), parse(version)) {
            (Some(min), Some(got)) => got >= min,
            _ => false,
        }
    } else {
        range.eq_ignore_ascii_case(version.trim())
    }
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

    #[test]
    #[ignore = "requires network access to the published release"]
    fn fetches_the_real_release_index() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (engines, source) = fetch(directory.path()).expect("fetch release index");
        assert_eq!(source, CatalogSource::Remote);
        assert!(!engines.is_empty());
        assert!(directory.path().join("index.json").exists());
    }

    #[test]
    #[ignore = "requires network and downloads a real engine archive"]
    fn installs_a_real_engine_archive() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (engines, _) = fetch(directory.path()).expect("fetch index");
        let engine = engines
            .into_iter()
            .find(|engine| engine.os == HostOs::current())
            .expect("host engine");
        let counter = Arc::new(AtomicU64::new(0));
        install(directory.path(), &engine, counter.clone()).expect("install engine");
        assert!(counter.load(Ordering::Relaxed) > 0);
        let bundle = reims_engine_mgr::engine_dir(directory.path(), &engine.engine_id);
        assert!(bundle.join("manifest.json").exists());
    }

    #[test]
    fn changelog_sections_become_commit_entries() {
        let raw = "# Engine changelog\nGenerated: 2026-11-28T00:00:00+00:00\n\n## abc123 · 2026-11-27T12:00:00Z\n### Fix video decode\n\nBody line one.\nBody line two.\n\n## def456 · 2026-11-26T12:00:00Z\n### Update QEMU\n";
        let entries = parse_changelog(raw);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sha, "abc123");
        assert_eq!(entries[0].subject, "Fix video decode");
        assert_eq!(entries[0].body, "Body line one.\nBody line two.\n");
        assert_eq!(entries[1].subject, "Update QEMU");
    }

    #[test]
    fn release_url_points_to_the_tag_page() {
        let engine = EngineRelease {
            engine_id: String::from("win64-master-a"),
            os: HostOs::Windows,
            channel: reims_core::contracts::Channel::Master,
            abi_version: 1,
            archive_sha256: "a".repeat(64),
            archive_size: 1,
            url: String::from(
                "https://github.com/owner/repo/releases/download/engines-master/a.tar.zst",
            ),
            supported_macos_range: String::from("26.4+"),
            changelog: None,
            built_at: None,
            source_revision: None,
            qemu_revision: None,
        };
        assert_eq!(
            release_url(&engine).as_deref(),
            Some("https://github.com/owner/repo/releases/tag/engines-master")
        );
    }

    #[test]
    fn macos_range_filters_builds() {
        let mut engine = EngineRelease {
            engine_id: String::from("win64-master-a"),
            os: HostOs::Windows,
            channel: reims_core::contracts::Channel::Master,
            abi_version: 1,
            archive_sha256: "a".repeat(64),
            archive_size: 1,
            url: String::from("https://example.invalid/a"),
            supported_macos_range: String::from("26.4+"),
            changelog: None,
            built_at: None,
            source_revision: None,
            qemu_revision: None,
        };
        assert!(supports_macos(&engine, "26.4"));
        assert!(supports_macos(&engine, "27.1"));
        assert!(!supports_macos(&engine, "26.3"));
        engine.supported_macos_range = String::from("26.4");
        assert!(supports_macos(&engine, "26.4"));
        assert!(!supports_macos(&engine, "26.5"));
    }
}
