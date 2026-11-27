//! Crash decoding driven by the external `signatures.json` dictionary.
//!
//! # Quick start
//!
//! ```no_run
//! use reims_core::contracts::{load_json, SignatureCatalog};
//! use reims_core::crash::CrashDecoder;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let catalog: SignatureCatalog = load_json(std::path::Path::new("signatures.json"))?;
//! let decoder = CrashDecoder::new(&catalog)?;
//! if let Some(crash) = decoder.decode("reims-vgpu: fatal: VK_DEVICE_LOST") {
//!     println!("{}: {}", crash.signature_id, crash.advice);
//! }
//! # Ok(())
//! # }
//! ```

use regex::Regex;

use crate::contracts::{ContractError, CrashSignature, Severity, SignatureCatalog, Validate};

#[derive(Debug)]
struct CompiledSignature {
    pattern: Regex,
    signature: CrashSignature,
}

/// One decoded failure with the log line that triggered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedCrash {
    /// Identifier from `signatures.json`.
    pub signature_id: String,
    /// Presentation severity.
    pub severity: Severity,
    /// Advice from `signatures.json`.
    pub advice: String,
    /// First matching log line.
    pub evidence: String,
    /// One-based index of the matching log line.
    pub line_number: usize,
}

/// Matches engine logs against the external signature dictionary.
#[derive(Debug)]
pub struct CrashDecoder {
    signatures: Vec<CompiledSignature>,
}

impl CrashDecoder {
    /// Compiles every pattern from `catalog`.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::Pattern`] for an invalid regular expression.
    pub fn new(catalog: &SignatureCatalog) -> Result<Self, ContractError> {
        catalog.validate()?;
        let mut signatures = Vec::with_capacity(catalog.signatures.len());
        for signature in &catalog.signatures {
            signatures.push(CompiledSignature {
                pattern: Regex::new(&signature.pattern).map_err(|source| {
                    ContractError::Pattern {
                        id: signature.id.clone(),
                        source,
                    }
                })?,
                signature: signature.clone(),
            });
        }
        Ok(Self { signatures })
    }

    /// Returns the highest-severity signature found in `log`.
    ///
    /// Ties keep the signature that appears earlier in the catalog. Patterns
    /// are matched line by line; the first matching line becomes the evidence.
    pub fn decode(&self, log: &str) -> Option<DecodedCrash> {
        let mut best: Option<DecodedCrash> = None;
        for compiled in &self.signatures {
            for (index, line) in log.lines().enumerate() {
                if !compiled.pattern.is_match(line) {
                    continue;
                }
                let decoded = DecodedCrash {
                    signature_id: compiled.signature.id.clone(),
                    severity: compiled.signature.severity,
                    advice: compiled.signature.advice.clone(),
                    evidence: line.trim().to_owned(),
                    line_number: index + 1,
                };
                if best
                    .as_ref()
                    .is_none_or(|current| decoded.severity > current.severity)
                {
                    best = Some(decoded);
                }
                break;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::SIGNATURES_SCHEMA_VERSION;

    fn catalog() -> SignatureCatalog {
        SignatureCatalog {
            schema_version: SIGNATURES_SCHEMA_VERSION,
            signatures: vec![
                CrashSignature {
                    id: String::from("verbose_boot"),
                    pattern: String::from("EXITBS:START"),
                    advice: String::from("Enable verbose boot."),
                    severity: Severity::Info,
                },
                CrashSignature {
                    id: String::from("device_lost"),
                    pattern: String::from("VK_DEVICE_LOST"),
                    advice: String::from("Isolate the GPU driver."),
                    severity: Severity::Critical,
                },
            ],
        }
    }

    #[test]
    fn critical_signature_wins_over_info() {
        let decoder = CrashDecoder::new(&catalog()).expect("decoder");
        let log = "EXITBS:START waiting for root device\nreims-vgpu: fatal: VK_DEVICE_LOST\n";
        let decoded = decoder.decode(log).expect("decoded crash");
        assert_eq!(decoded.signature_id, "device_lost");
        assert_eq!(decoded.line_number, 2);
        assert_eq!(decoded.severity, Severity::Critical);
    }

    #[test]
    fn clean_log_returns_none() {
        let decoder = CrashDecoder::new(&catalog()).expect("decoder");
        assert!(decoder.decode("boot successful").is_none());
    }

    #[test]
    fn decoder_rejects_an_invalid_catalog() {
        let mut broken = catalog();
        broken.signatures[0].pattern = String::from("(");
        assert!(CrashDecoder::new(&broken).is_err());
    }
}
