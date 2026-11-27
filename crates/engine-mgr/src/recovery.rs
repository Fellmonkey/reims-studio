//! Apple Recovery request data and RSA chunklist handling.
//!
//! The CDN client itself stays outside this crate: the launcher supplies the
//! downloaded chunk bytes. This module parses the chunklist and verifies each
//! chunk digest before the DMG is assembled.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use plist::Value;
use sha2::{Digest, Sha256};

use crate::manager::EngineError;

/// Recovery image endpoint.
pub const RECOVERY_URL: &str = "https://osrecovery.apple.com/InstallationPayload/RecoveryImage";
/// Software update catalog endpoint.
pub const SWSCAN_URL: &str =
    "https://swscan.apple.com/content/catalogs/others/index-10.16-1.sucatalog";

/// Identity supplied to the Apple Recovery endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRequest {
    /// Board-ID of the target Mac model.
    pub board_id: String,
    /// Logic board serial number.
    pub mlb: String,
    /// Asset token obtained from the catalog.
    pub asset_token: String,
}

impl RecoveryRequest {
    /// Checks that every field is non-empty and free of control characters.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidArchive`] for an invalid field.
    pub fn validate(&self) -> Result<(), EngineError> {
        for (name, value) in [
            ("board_id", &self.board_id),
            ("mlb", &self.mlb),
            ("asset_token", &self.asset_token),
        ] {
            if value.is_empty() || value.chars().any(char::is_control) {
                return Err(EngineError::InvalidArchive(format!(
                    "recovery {name} is empty or contains control characters"
                )));
            }
        }
        Ok(())
    }

    /// Returns the headers derived from the request.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidArchive`] when the request is invalid.
    pub fn headers(&self) -> Result<Vec<(String, String)>, EngineError> {
        self.validate()?;
        Ok(vec![
            (
                String::from("Cookie"),
                format!("AssetToken={}", self.asset_token),
            ),
            (String::from("X-Apple-Board-ID"), self.board_id.clone()),
            (String::from("X-Apple-MLB"), self.mlb.clone()),
        ])
    }
}

/// One verified chunk of the recovery image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Hexadecimal or base64 SHA256 signature.
    pub signature: String,
    /// Expected uncompressed size in bytes.
    pub size: u64,
}

/// Parsed Apple chunklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkList {
    /// Base URL for chunk downloads, when present.
    pub base_url: Option<String>,
    /// Chunks in download order.
    pub chunks: Vec<Chunk>,
}

fn dictionary<'a>(value: &'a Value, key: &str) -> Result<&'a Value, EngineError> {
    value
        .as_dictionary()
        .and_then(|dictionary| dictionary.get(key))
        .ok_or_else(|| EngineError::InvalidArchive(format!("chunklist missing {key}")))
}

/// Parses an XML chunklist plist.
///
/// # Errors
///
/// Returns [`EngineError::InvalidArchive`] when the plist is malformed or
/// lacks chunk signatures.
pub fn parse_chunklist(xml: &str) -> Result<ChunkList, EngineError> {
    let root: Value = plist::from_bytes(xml.as_bytes())
        .map_err(|error| EngineError::InvalidArchive(format!("bad chunklist plist: {error}")))?;
    let base_url = dictionary(&root, "baseURL")
        .ok()
        .and_then(Value::as_string)
        .map(str::to_owned);
    let chunks_value = dictionary(&root, "chunks")?;
    let entries = chunks_value.as_array().ok_or_else(|| {
        EngineError::InvalidArchive(String::from("chunklist chunks is not an array"))
    })?;
    let mut chunks = Vec::with_capacity(entries.len());
    for entry in entries {
        let signature = dictionary(entry, "signature")?
            .as_string()
            .map(str::to_owned)
            .ok_or_else(|| EngineError::InvalidArchive(String::from("chunk has no signature")))?;
        let size = dictionary(entry, "size")
            .ok()
            .and_then(Value::as_signed_integer)
            .map(|value| value as u64)
            .unwrap_or_default();
        chunks.push(Chunk { signature, size });
    }
    Ok(ChunkList { base_url, chunks })
}

fn signature_bytes(signature: &str) -> Result<Vec<u8>, EngineError> {
    if signature.len() == 64 && signature.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let mut bytes = Vec::with_capacity(32);
        for pair in signature.as_bytes().as_chunks::<2>().0 {
            let text = std::str::from_utf8(pair).map_err(|_| {
                EngineError::InvalidSignature(String::from("non-UTF-8 hex signature"))
            })?;
            bytes.push(
                u8::from_str_radix(text, 16).map_err(|_| {
                    EngineError::InvalidSignature(String::from("bad hex signature"))
                })?,
            );
        }
        return Ok(bytes);
    }
    BASE64
        .decode(signature)
        .map_err(|error| EngineError::InvalidSignature(error.to_string()))
}

/// Verifies one chunk against its declared SHA256 signature.
///
/// # Errors
///
/// Returns [`EngineError::InvalidSignature`] for a bad encoding or digest.
pub fn verify_chunk(data: &[u8], signature: &str) -> Result<(), EngineError> {
    let expected = signature_bytes(signature)?;
    let digest = Sha256::digest(data);
    if expected != digest.as_slice() {
        return Err(EngineError::InvalidSignature(String::from(
            "digest mismatch",
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>baseURL</key><string>https://example.invalid/chunks</string>
  <key>chunks</key><array>
    <dict><key>signature</key><string>ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad</string><key>size</key><integer>3</integer></dict>
  </array>
</dict></plist>"#;

    #[test]
    fn parses_the_chunklist() {
        let list = parse_chunklist(PLIST).expect("chunklist");
        assert_eq!(list.chunks.len(), 1);
        assert_eq!(list.chunks[0].size, 3);
        assert_eq!(
            list.base_url.as_deref(),
            Some("https://example.invalid/chunks")
        );
    }

    #[test]
    fn verifies_a_chunk_digest() {
        let list = parse_chunklist(PLIST).expect("chunklist");
        verify_chunk(b"abc", &list.chunks[0].signature).expect("valid chunk");
        assert!(verify_chunk(b"abd", &list.chunks[0].signature).is_err());
    }

    #[test]
    fn rejects_bad_recovery_identities() {
        let request = RecoveryRequest {
            board_id: String::from("Mac-123"),
            mlb: String::new(),
            asset_token: String::from("token"),
        };
        assert!(request.headers().is_err());
    }

    #[test]
    fn accepts_base64_signatures() {
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let encoded = BASE64.encode(signature_bytes(digest).expect("hex"));
        verify_chunk(b"abc", &encoded).expect("base64 chunk");
    }
}
