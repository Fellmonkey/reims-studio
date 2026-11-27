//! Declarative OpenCore configuration and guest asset assembly.
//!
//! Names come from the engine bundle and `guest/prebuilt/set.toml`. Rust code
//! patches only known OpenCore keys and never hardcodes kext names.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use plist::Value;
use reims_core::contracts::{ContractError, SmbiosConfig};

use crate::manager::EngineError;

const BOOT_ARGS_KEY: &str = "7C436110-AB2A-4BBB-A880-FE41995C9F82";

/// Two-phase OpenCore settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcPatch {
    /// OpenCore `Misc.Security.ScanPolicy`.
    pub scan_policy: u32,
    /// OpenCore `Misc.Boot.Timeout` in seconds.
    pub timeout: u32,
    /// Whether the boot picker is shown.
    pub show_picker: bool,
    /// Whether `-v` is appended to boot arguments.
    pub verbose: bool,
    /// SMBIOS identity copied from `smbios.toml`, when provided.
    pub smbios: Option<SmbiosConfig>,
}

impl OcPatch {
    /// Returns the install-phase settings: `983811`, timeout 5, picker on.
    pub fn install() -> Self {
        Self {
            scan_policy: 983_811,
            timeout: 5,
            show_picker: true,
            verbose: false,
            smbios: None,
        }
    }

    /// Returns the post-install settings: `983299`, timeout 2, picker on.
    pub fn post_install() -> Self {
        Self {
            scan_policy: 983_299,
            timeout: 2,
            show_picker: true,
            verbose: false,
            smbios: None,
        }
    }

    /// Returns the Safe Mode settings with verbose boot enabled.
    pub fn diagnostics() -> Self {
        Self {
            verbose: true,
            ..Self::install()
        }
    }
}

fn child<'a>(value: &'a mut Value, key: &str) -> Result<&'a mut Value, EngineError> {
    let dictionary = value
        .as_dictionary_mut()
        .ok_or_else(|| EngineError::InvalidArchive(format!("expected a dictionary at {key}")))?;
    dictionary
        .get_mut(key)
        .ok_or_else(|| EngineError::InvalidArchive(format!("missing OpenCore key {key}")))
}

fn set_integer(value: &mut Value, key: &str, number: u32) -> Result<(), EngineError> {
    *child(value, key)? = Value::Integer(number.into());
    Ok(())
}

fn set_boolean(value: &mut Value, key: &str, enabled: bool) -> Result<(), EngineError> {
    *child(value, key)? = Value::Boolean(enabled);
    Ok(())
}

/// Patches a parsed OpenCore `config.plist` in place.
///
/// # Errors
///
/// Returns [`EngineError::InvalidArchive`] when a required dictionary is
/// missing or has the wrong type.
pub fn patch_config(config: &mut Value, patch: &OcPatch) -> Result<(), EngineError> {
    let misc = child(config, "Misc")?;
    let security = child(misc, "Security")?;
    set_integer(security, "ScanPolicy", patch.scan_policy)?;
    let boot = child(misc, "Boot")?;
    set_integer(boot, "Timeout", patch.timeout)?;
    set_boolean(boot, "ShowPicker", patch.show_picker)?;

    if patch.verbose {
        let nvram = child(config, "NVRAM")?;
        let add = child(nvram, "Add")?;
        let boot_args = child(add, BOOT_ARGS_KEY)?;
        let Value::String(text) = boot_args else {
            return Err(EngineError::InvalidArchive(String::from(
                "boot-args is not a string",
            )));
        };
        if !text.split_whitespace().any(|argument| argument == "-v") {
            text.push_str(" -v");
        }
    }

    if let Some(smbios) = &patch.smbios {
        let platform = child(config, "PlatformInfo")?;
        let generic = child(platform, "Generic")?;
        let dictionary = generic.as_dictionary_mut().ok_or_else(|| {
            EngineError::InvalidArchive(String::from("PlatformInfo.Generic is not a dictionary"))
        })?;
        dictionary.insert(
            String::from("SystemSerialNumber"),
            Value::String(smbios.serial.clone()),
        );
        dictionary.insert(
            String::from("SystemProductName"),
            Value::String(smbios.model.clone()),
        );
    }
    Ok(())
}

fn kext_names(value: &toml::Value) -> Vec<String> {
    let mut found = Vec::new();
    match value {
        toml::Value::String(text) if text.contains(".kext") => found.push(text.clone()),
        toml::Value::Table(table) => {
            for item in table.values() {
                found.extend(kext_names(item));
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                found.extend(kext_names(item));
            }
        }
        _ => {}
    }
    found
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), EngineError> {
    std::fs::create_dir_all(destination).map_err(|source_error| EngineError::Io {
        path: destination.display().to_string(),
        source: source_error,
    })?;
    for entry in std::fs::read_dir(source).map_err(|source_error| EngineError::Io {
        path: source.display().to_string(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| EngineError::Io {
            path: source.display().to_string(),
            source: source_error,
        })?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|source_error| EngineError::Io {
            path: target.display().to_string(),
            source: source_error,
        })?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|source_error| EngineError::Io {
                path: target.display().to_string(),
                source: source_error,
            })?;
        }
    }
    Ok(())
}

/// Copies every kext named in `set.toml` into the EFI folder recursively.
///
/// # Errors
///
/// Returns [`EngineError`] on an unsafe path, a malformed `set.toml` or a
/// filesystem failure.
pub fn copy_kexts(
    set_toml: &Path,
    guest_root: &Path,
    esp_root: &Path,
) -> Result<Vec<String>, EngineError> {
    let raw = std::fs::read_to_string(set_toml).map_err(|source| EngineError::Io {
        path: set_toml.display().to_string(),
        source,
    })?;
    let parsed: toml::Value = toml::from_str(&raw).map_err(|source| {
        EngineError::Contract(ContractError::Toml {
            path: set_toml.display().to_string(),
            source,
        })
    })?;
    let destination = esp_root.join("EFI/OC/Kexts");
    let mut copied = Vec::new();
    for name in kext_names(&parsed) {
        let relative = Path::new(&name);
        if relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(EngineError::UnsafePath(name));
        }
        let Some(kext_name) = relative.components().next().and_then(|part| match part {
            Component::Normal(value) => Some(value),
            _ => None,
        }) else {
            return Err(EngineError::UnsafePath(name));
        };
        // set.toml may name a single file inside Contents; always copy the
        // whole kext tree so Metal bundles and resources are included.
        let source = guest_root.join(kext_name);
        copy_tree(&source, &destination.join(kext_name))?;
        copied.push(name);
    }
    Ok(copied)
}

/// Checks that the writable OVMF variables file matches the code file.
///
/// # Errors
///
/// Returns [`EngineError::InvalidArchive`] when either file is missing or
/// their sizes differ.
pub fn check_ovmf_pair(code: &Path, variables: &Path) -> Result<(), EngineError> {
    let code_size = std::fs::metadata(code)
        .map_err(|source| EngineError::Io {
            path: code.display().to_string(),
            source,
        })?
        .len();
    let vars_size = std::fs::metadata(variables)
        .map_err(|source| EngineError::Io {
            path: variables.display().to_string(),
            source,
        })?
        .len();
    if code_size == 0 || code_size != vars_size {
        return Err(EngineError::InvalidArchive(String::from(
            "OVMF_CODE and OVMF_VARS sizes do not match",
        )));
    }
    Ok(())
}

/// Returns the dictionaries patched by [`patch_config`], for diagnostics.
pub fn patched_paths() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("scan_policy", "Misc.Security.ScanPolicy"),
        ("timeout", "Misc.Boot.Timeout"),
        ("show_picker", "Misc.Boot.ShowPicker"),
        ("boot_args", "NVRAM.Add.boot-args"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(entries: &[(&str, Value)]) -> Value {
        let mut dictionary = plist::Dictionary::new();
        for (key, value) in entries {
            dictionary.insert((*key).to_owned(), value.clone());
        }
        Value::Dictionary(dictionary)
    }

    fn integer(value: u32) -> Value {
        Value::Integer(value.into())
    }

    fn config() -> Value {
        dict(&[
            (
                "Misc",
                dict(&[
                    ("Security", dict(&[("ScanPolicy", integer(0))])),
                    (
                        "Boot",
                        dict(&[
                            ("Timeout", integer(5)),
                            ("ShowPicker", Value::Boolean(false)),
                        ]),
                    ),
                ]),
            ),
            (
                "NVRAM",
                dict(&[(
                    "Add",
                    dict(&[(BOOT_ARGS_KEY, Value::String(String::from("npci=0x3000")))]),
                )]),
            ),
            ("PlatformInfo", dict(&[("Generic", dict(&[]))])),
        ])
    }

    #[test]
    fn install_patch_sets_the_install_scan_policy() {
        let mut config = config();
        patch_config(&mut config, &OcPatch::install()).expect("patch");
        let misc = config
            .as_dictionary()
            .expect("misc")
            .get("Misc")
            .expect("misc");
        let security = misc
            .as_dictionary()
            .expect("dict")
            .get("Security")
            .expect("security");
        assert_eq!(
            security.as_dictionary().expect("dict").get("ScanPolicy"),
            Some(&integer(983_811))
        );
    }

    #[test]
    fn post_install_patch_changes_timeout_and_scan_policy() {
        let mut config = config();
        patch_config(&mut config, &OcPatch::post_install()).expect("patch");
        let misc = config
            .as_dictionary()
            .expect("root")
            .get("Misc")
            .expect("misc");
        let boot = misc
            .as_dictionary()
            .expect("dict")
            .get("Boot")
            .expect("boot");
        let boot = boot.as_dictionary().expect("dict");
        assert_eq!(boot.get("Timeout"), Some(&integer(2)));
        assert_eq!(boot.get("ShowPicker"), Some(&Value::Boolean(true)));
    }

    #[test]
    fn diagnostics_patch_appends_verbose_once() {
        let mut config = config();
        let patch = OcPatch::diagnostics();
        patch_config(&mut config, &patch).expect("first patch");
        patch_config(&mut config, &patch).expect("second patch");
        let nvram = config
            .as_dictionary()
            .expect("root")
            .get("NVRAM")
            .expect("nvram");
        let add = nvram
            .as_dictionary()
            .expect("dict")
            .get("Add")
            .expect("add");
        let args = add
            .as_dictionary()
            .expect("dict")
            .get(BOOT_ARGS_KEY)
            .expect("args");
        assert_eq!(args.as_string(), Some("npci=0x3000 -v"));
    }

    #[test]
    fn ovmf_pair_check_rejects_mismatched_sizes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let code = dir.path().join("OVMF_CODE.fd");
        let vars = dir.path().join("OVMF_VARS.fd");
        std::fs::write(&code, vec![0_u8; 1024]).expect("write code");
        std::fs::write(&vars, vec![0_u8; 512]).expect("write vars");
        assert!(check_ovmf_pair(&code, &vars).is_err());
        std::fs::write(&vars, vec![0_u8; 1024]).expect("rewrite vars");
        check_ovmf_pair(&code, &vars).expect("matching pair");
    }

    #[test]
    fn kexts_are_copied_recursively() {
        let dir = tempfile::tempdir().expect("temp dir");
        let set = dir.path().join("set.toml");
        std::fs::write(&set, "kexts = ['Graft.kext/Contents/Info.plist']\n").expect("set.toml");
        let source = dir.path().join("guest/Graft.kext/Contents");
        std::fs::create_dir_all(&source).expect("source dir");
        std::fs::write(source.join("Info.plist"), b"plist").expect("kext file");
        let copied = copy_kexts(&set, &dir.path().join("guest"), &dir.path().join("esp"))
            .expect("copy kexts");
        assert_eq!(copied.len(), 1);
        assert!(
            dir.path()
                .join("esp/EFI/OC/Kexts/Graft.kext/Contents/Info.plist")
                .exists()
        );
    }

    #[test]
    fn unsafe_kext_paths_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let set = dir.path().join("set.toml");
        std::fs::write(&set, "kexts = ['../Escape.kext']\n").expect("set.toml");
        assert!(matches!(
            copy_kexts(&set, dir.path(), &dir.path().join("esp")),
            Err(EngineError::UnsafePath(_))
        ));
    }
}
