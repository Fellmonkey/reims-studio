//! Checks the named regression registry required by change acceptance.

use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn regression_registry_is_well_formed() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/regressions.tsv");
    let raw = std::fs::read_to_string(&path).expect("read regression registry");
    let mut names = BTreeSet::new();
    for (index, line) in raw.lines().enumerate() {
        let (name, reason) = line.split_once('\t').unwrap_or_else(|| {
            panic!("line {} of regressions.tsv has no tab separator", index + 1)
        });
        assert!(
            !name.is_empty(),
            "empty regression name on line {}",
            index + 1
        );
        assert!(
            !reason.trim().is_empty(),
            "empty regression reason on line {}",
            index + 1
        );
        assert!(names.insert(name), "duplicate regression name: {name}");
    }
    assert!(names.contains("local/001"));
    assert!(names.contains("local/002"));
}
