//! Forensic DMG audit and conversion to a read-only disk image.

use std::io;
use std::path::Path;

use dmg::DmgReader;
use dmg_forensic::audit_path;
use forensicnomicon::report::Severity;

use crate::manager::EngineError;

/// Result of the `koly`-strictness audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditReport {
    /// Whether conversion may proceed.
    pub allowed: bool,
    /// High or Critical findings that stop the pipeline.
    pub blockers: Vec<String>,
    /// Informational or low-severity findings.
    pub notes: Vec<String>,
}

/// Audits a DMG with `dmg-forensic`.
///
/// Only `High` and `Critical` findings block conversion; lower severities are
/// kept as notes.
///
/// # Errors
///
/// Returns [`EngineError::Io`] when the file cannot be read.
pub fn audit_dmg(path: &Path) -> Result<AuditReport, EngineError> {
    let anomalies = audit_path(path).map_err(|source| EngineError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let mut report = AuditReport {
        allowed: true,
        blockers: Vec::new(),
        notes: Vec::new(),
    };
    for anomaly in anomalies {
        let line = format!("{} {}: {}", anomaly.severity, anomaly.code, anomaly.note);
        if anomaly.severity >= Severity::High {
            report.blockers.push(line);
            report.allowed = false;
        } else {
            report.notes.push(line);
        }
    }
    Ok(report)
}

/// Converts an audited DMG into a raw read-only image.
///
/// # Errors
///
/// Returns [`EngineError::DmgAudit`] when the audit blocked the file, an I/O
/// error when either file cannot be opened, or
/// [`EngineError::InvalidArchive`] when `dmg-core` cannot parse the image.
pub fn to_readonly_image(
    dmg_path: &Path,
    image_path: &Path,
    audit: &AuditReport,
) -> Result<u64, EngineError> {
    if !audit.allowed {
        return Err(EngineError::DmgAudit(audit.blockers.clone()));
    }
    let file = std::fs::File::open(dmg_path).map_err(|source| EngineError::Io {
        path: dmg_path.display().to_string(),
        source,
    })?;
    let mut reader = DmgReader::open(file)
        .map_err(|error| EngineError::InvalidArchive(format!("dmg-core: {error}")))?;
    let mut output = std::fs::File::create(image_path).map_err(|source| EngineError::Io {
        path: image_path.display().to_string(),
        source,
    })?;
    let copied = io::copy(&mut reader, &mut output).map_err(|source| EngineError::Io {
        path: image_path.display().to_string(),
        source,
    })?;
    output.sync_all().map_err(|source| EngineError::Io {
        path: image_path.display().to_string(),
        source,
    })?;
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_dmg_files_are_blocked() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("BaseSystem.dmg");
        std::fs::write(&path, b"not a dmg").expect("write file");
        let report = audit_dmg(&path).expect("audit");
        assert!(!report.allowed);
        assert!(!report.blockers.is_empty());
    }

    #[test]
    fn conversion_rejects_a_blocked_audit() {
        let directory = tempfile::tempdir().expect("temp dir");
        let report = AuditReport {
            allowed: false,
            blockers: vec![String::from("HIGH TEST")],
            notes: Vec::new(),
        };
        let result = to_readonly_image(
            &directory.path().join("in.dmg"),
            &directory.path().join("readonly.img"),
            &report,
        );
        assert!(matches!(result, Err(EngineError::DmgAudit(_))));
    }
}
