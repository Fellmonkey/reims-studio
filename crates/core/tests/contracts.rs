//! Integration checks for the versioned contract fixtures.

use std::path::{Path, PathBuf};

use reims_core::contracts::{
    EngineIndex, EngineManifest, HostProfile, Settings, SignatureCatalog, SmbiosConfig, VmConfig,
    load_json, load_toml,
};
use reims_core::crash::CrashDecoder;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn every_contract_fixture_loads() {
    let manifest: EngineManifest = load_json(&fixture("manifest.json")).expect("manifest");
    assert_eq!(manifest.abi_version, 1);
    assert_eq!(manifest.guest_kexts.len(), 3);

    let index: EngineIndex = load_json(&fixture("index.json")).expect("index");
    assert_eq!(index.engines.len(), 2);

    let settings: Settings = load_toml(&fixture("settings.toml")).expect("settings");
    assert_eq!(settings.bridge_port, 8080);

    let vm: VmConfig = load_toml(&fixture("vm.toml")).expect("vm config");
    assert_eq!(vm.scan_policy, 983_811);

    let smbios: SmbiosConfig = load_toml(&fixture("smbios.toml")).expect("smbios");
    assert_eq!(smbios.model, "iMac20,1");

    let profile: HostProfile = load_toml(&fixture("host_profile.toml")).expect("host profile");
    assert!(profile.probe_done);

    let catalog: SignatureCatalog = load_json(&fixture("signatures.json")).expect("catalog");
    assert_eq!(catalog.signatures.len(), 3);
}

#[test]
fn fixture_catalog_decodes_a_fatal_line() {
    let catalog: SignatureCatalog = load_json(&fixture("signatures.json")).expect("catalog");
    let decoder = CrashDecoder::new(&catalog).expect("decoder");
    let decoded = decoder
        .decode("reims-vgpu: fatal: VK_DEVICE_LOST on device 0")
        .expect("decoded crash");
    assert_eq!(decoded.signature_id, "device_lost");
    assert!(decoded.advice.contains("GPU isolation"));
}
