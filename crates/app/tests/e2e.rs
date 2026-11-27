//! End-to-end check: the built launcher must see a built engine bundle.

// Fixture setup failures are test failures; expect is the clearest signal.
#![allow(clippy::expect_used, reason = "test fixture setup")]

use std::path::Path;

use reims_core::contracts::VmConfig;

const ARCHIVE_SHA256: &str = "707748c22ceaddb5a1057403c94a835994a72bbebfbec8bca82c0beb00fa8313";

fn engine_id() -> String {
    if cfg!(windows) {
        String::from("win64-development-e2e")
    } else {
        String::from("linux64-development-e2e")
    }
}

fn os_name() -> &'static str {
    if cfg!(windows) { "windows" } else { "linux" }
}

fn write(path: &Path, content: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create fixture directory");
    }
    std::fs::write(path, content).expect("write fixture");
}

fn fixture(root: &Path) {
    let id = engine_id();
    write(
        &root.join("settings.toml"),
        b"schema_version = 1\nchannel = \"development\"\nbridge_port = 8080\nqmp_port = 4444\n",
    );
    write(
        &root.join("index.json"),
        format!(
            r#"{{"schema_version":1,"engines":[{{"engine_id":"{id}","os":"{}","channel":"development","abi_version":1,"archive_sha256":"{ARCHIVE_SHA256}","archive_size":14,"url":"https://example.invalid/engine.tar.zst","supported_macos_range":"26.4+"}}]}}"#,
            os_name()
        )
        .as_bytes(),
    );
    write(
        &root.join("engines").join(&id).join("manifest.json"),
        format!(
            r#"{{
  "schema_version": 1,
  "abi_version": 1,
  "engine_id": "{id}",
  "os": "{}",
  "channel": "development",
  "source_revision": "e2e",
  "qemu_revision": "e2e",
  "rom_filename": "ReimsGraphics.rom",
  "video_prop_name": "video-disable",
  "ovmf_code_size": 2097152,
  "supported_macos_range": "26.4+",
  "va_driver_name": null,
  "va_drivers_relpath": null,
  "files": {{}},
  "guest_kexts": []
}}"#,
            os_name()
        )
        .as_bytes(),
    );
    write(
        &root.join("cache").join(format!("{id}.tar.zst")),
        b"engine archive",
    );
    let vm = toml::to_string(&VmConfig::new("macos-tahoe")).expect("serialize vm config");
    write(&root.join("vms/macos-tahoe/vm.toml"), vm.as_bytes());
}

fn run(root: &Path, mode: &str) -> (bool, String) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_reims-studio"))
        .arg(mode)
        .env("REIMS_PORTABLE_DIR", root)
        .output()
        .expect("run launcher");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn launcher_sees_the_built_engine() {
    let directory = tempfile::tempdir().expect("temp dir");
    fixture(directory.path());

    let (ok, output) = run(directory.path(), "--check");
    assert!(ok, "check failed: {output}");
    assert!(output.contains(&format!("engine: {}", engine_id())));
    assert!(output.contains("abi: 1"));
    assert!(output.contains("channel: development"));
    assert!(output.contains("--check: OK"));

    let (ok, output) = run(directory.path(), "--e2e");
    assert!(ok, "e2e failed: {output}");
    assert!(output.contains("--e2e: OK"));
    assert!(output.contains("bridge_port: 8080"));
}

#[test]
fn e2e_detects_a_corrupted_archive() {
    let directory = tempfile::tempdir().expect("temp dir");
    fixture(directory.path());
    write(
        &directory
            .path()
            .join("cache")
            .join(format!("{}.tar.zst", engine_id())),
        b"tampered",
    );

    let (ok, output) = run(directory.path(), "--e2e");
    assert!(!ok, "corrupted archive was accepted: {output}");
    assert!(output.contains("--e2e: FAIL"));

    let (ok, output) = run(directory.path(), "--check");
    assert!(ok, "check should not require the archive: {output}");
}
