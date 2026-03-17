//! Snapshot test for the OpenAPI specification.
//!
//! This test ensures that changes to the OpenAPI spec are intentional and reviewed.
//! If the spec changes, the test fails with instructions on how to update the snapshot.

use strom::openapi::openapi_spec;

#[test]
fn openapi_spec_snapshot() {
    let spec = openapi_spec();
    let json = spec
        .to_pretty_json()
        .expect("Failed to serialize OpenAPI spec");

    let snapshot_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("openapi_snapshot.json");

    if !snapshot_path.exists() {
        std::fs::write(&snapshot_path, &json).expect("Failed to write snapshot");
        panic!(
            "OpenAPI snapshot did not exist and has been created at {}. \
             Commit it and re-run the test.",
            snapshot_path.display()
        );
    }

    let existing = std::fs::read_to_string(&snapshot_path).expect("Failed to read snapshot");

    if existing != json {
        let new_path = snapshot_path.with_extension("json.new");
        std::fs::write(&new_path, &json).expect("Failed to write new snapshot");
        panic!(
            "OpenAPI spec has changed!\n\
             Review the diff:\n  diff {} {}\n\
             If the change is intentional, update the snapshot:\n  \
             cp {} {}",
            snapshot_path.display(),
            new_path.display(),
            new_path.display(),
            snapshot_path.display()
        );
    }
}
