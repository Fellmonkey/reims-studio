//! Release selection and SHA256 verification.
//!
//! # Quick start
//!
//! ```
//! use reims_core::contracts::{Channel, EngineIndex, HostOs};
//! use reims_engine_mgr::select_release;
//!
//! let index = EngineIndex { schema_version: 1, engines: Vec::new() };
//! assert!(select_release(&index, HostOs::Linux, Channel::Master).is_none());
//! ```

use std::io::Read;
use std::path::Path;

use reims_core::contracts::{Channel, EngineIndex, EngineRelease, HostOs};
use sha2::{Digest, Sha256};
use thiserror::Error;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// Failures of engine selection, verification and installation.
#[derive(Debug, Error)]
pub enum EngineError {
    /// A versioned contract failed validation.
    #[error(transparent)]
    Contract(#[from] reims_core::contracts::ContractError),
    /// A file operation failed.
    #[error("cannot access {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The archive digest does not match the release index.
    #[error("SHA256 mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        /// Digest from `index.json`.
        expected: String,
        /// Digest computed from the file.
        actual: String,
    },
    /// The archive contains an unsafe path.
    #[error("unsafe archive path: {0}")]
    UnsafePath(String),
    /// The archive format is invalid.
    #[error("invalid engine archive: {0}")]
    InvalidArchive(String),
    /// The engine does not support the requested macOS version.
    #[error("engine supports {range}, not {version}")]
    UnsupportedMacos {
        /// Declared support range.
        range: String,
        /// Requested version.
        version: String,
    },
    /// The DMG audit reported High or Critical anomalies.
    #[error("DMG audit blocked the image: {}", .0.join("; "))]
    DmgAudit(Vec<String>),
    /// A chunk signature could not be decoded.
    #[error("invalid chunk signature: {0}")]
    InvalidSignature(String),
}

/// Returns the first release matching host and channel.
///
/// `index.json` is curated by release automation; its order defines priority.
///
/// # Errors
///
/// The index is assumed to be validated by
/// [`reims_core::contracts::Validate::validate`].
pub fn select_release(index: &EngineIndex, os: HostOs, channel: Channel) -> Option<&EngineRelease> {
    index
        .engines
        .iter()
        .find(|release| release.os == os && release.channel == channel)
}

/// Returns true when `range` from `index.json` contains `version`.
///
/// Supported forms are `26.4+` and `26.4-27.9`.
///
/// # Errors
///
/// Returns [`EngineError::UnsupportedMacos`] when either value cannot be parsed.
pub fn supports_macos(range: &str, version: &str) -> Result<bool, EngineError> {
    let requested = parse_version(version)?;
    let supported = if let Some(minimum) = range.strip_suffix('+') {
        let minimum = parse_version(minimum)?;
        requested >= minimum
    } else if let Some((minimum, maximum)) = range.split_once('-') {
        let minimum = parse_version(minimum)?;
        let maximum = parse_version(maximum)?;
        requested >= minimum && requested <= maximum
    } else {
        let exact = parse_version(range)?;
        requested == exact
    };
    Ok(supported)
}

fn parse_version(value: &str) -> Result<(u32, u32), EngineError> {
    let (major, minor) = value
        .split_once('.')
        .ok_or_else(|| EngineError::UnsupportedMacos {
            range: value.to_owned(),
            version: value.to_owned(),
        })?;
    let major = major.parse().map_err(|_| EngineError::UnsupportedMacos {
        range: value.to_owned(),
        version: value.to_owned(),
    })?;
    let minor = minor.parse().map_err(|_| EngineError::UnsupportedMacos {
        range: value.to_owned(),
        version: value.to_owned(),
    })?;
    Ok((major, minor))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

/// Computes the lowercase SHA256 digest of a file.
///
/// # Errors
///
/// Returns [`EngineError::Io`] when the file cannot be read.
pub fn sha256_file(path: &Path) -> Result<String, EngineError> {
    let mut file = std::fs::File::open(path).map_err(|source| EngineError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| EngineError::Io {
            path: path.display().to_string(),
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Verifies a downloaded archive against `index.json` before unpacking.
///
/// # Errors
///
/// Returns [`EngineError::HashMismatch`] for a corrupted or replaced asset.
pub fn verify_archive(path: &Path, expected: &str) -> Result<(), EngineError> {
    let actual = sha256_file(path)?;
    if actual != expected.to_ascii_lowercase() {
        return Err(EngineError::HashMismatch {
            expected: expected.to_owned(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Write as _;

    use reims_core::contracts::{
        EngineManifest, INDEX_SCHEMA_VERSION, MANIFEST_SCHEMA_VERSION, Validate,
    };

    use super::*;

    #[test]
    fn selects_the_first_matching_release() {
        let index = EngineIndex {
            schema_version: INDEX_SCHEMA_VERSION,
            engines: vec![
                EngineRelease {
                    engine_id: String::from("win64-development-a"),
                    os: HostOs::Windows,
                    channel: Channel::Development,
                    abi_version: 1,
                    archive_sha256: String::new(),
                    archive_size: 1,
                    url: String::from("https://example.invalid/a"),
                    supported_macos_range: String::from("26.4+"),
                },
                EngineRelease {
                    engine_id: String::from("linux64-master-b"),
                    os: HostOs::Linux,
                    channel: Channel::Master,
                    abi_version: 1,
                    archive_sha256: String::new(),
                    archive_size: 1,
                    url: String::from("https://example.invalid/b"),
                    supported_macos_range: String::from("26.4+"),
                },
            ],
        };
        let selected = select_release(&index, HostOs::Windows, Channel::Development)
            .expect("windows development release");
        assert_eq!(selected.engine_id, "win64-development-a");
        assert!(select_release(&index, HostOs::Linux, Channel::Patched).is_none());
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("payload.bin");
        let mut file = std::fs::File::create(&path).expect("create file");
        file.write_all(b"abc").expect("write file");
        drop(file);
        let digest = sha256_file(&path).expect("digest");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        verify_archive(&path, &digest).expect("verified");
        assert!(matches!(
            verify_archive(&path, &"0".repeat(64)),
            Err(EngineError::HashMismatch { .. })
        ));
    }

    #[test]
    fn macos_ranges_are_checked() {
        assert!(supports_macos("26.4+", "27.1").expect("range"));
        assert!(!supports_macos("26.4+", "26.3").expect("range"));
        assert!(supports_macos("26.4-27.9", "27.0").expect("range"));
        assert!(!supports_macos("26.4-27.9", "28.0").expect("range"));
        assert!(supports_macos("26.4", "26.4").expect("range"));
    }

    #[test]
    fn manifest_fixture_still_validates() {
        let manifest = EngineManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            abi_version: 1,
            engine_id: String::from("linux64-master-a"),
            os: HostOs::Linux,
            channel: Channel::Master,
            source_revision: String::from("a"),
            qemu_revision: String::from("b"),
            rom_filename: String::from("Test.rom"),
            video_prop_name: String::from("video-disable"),
            ovmf_code_size: 1,
            supported_macos_range: String::from("26.4+"),
            va_driver_name: None,
            va_drivers_relpath: None,
            files: BTreeMap::new(),
            guest_kexts: Vec::new(),
        };
        manifest.validate().expect("valid manifest");
    }
}
