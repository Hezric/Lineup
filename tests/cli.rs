use serde_json::Value;
use std::process::Command;

#[test]
fn every_help_and_input_failure_is_one_json_document() {
    for args in [
        vec![],
        vec!["--help"],
        vec!["--version"],
        vec!["invalid"],
        vec!["list", "unexpected"],
        vec!["plan", "--manifest", "does-not-exist.json"],
        vec!["apply", "--manifest", "tests/fixtures/invalid.json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_lineup"))
            .args(&args)
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let expected = if args.is_empty() || args[0].starts_with("--") {
            0
        } else {
            2
        };
        assert_eq!(output.status.code(), Some(expected), "{args:?}: {value}");
    }
    for command in ["list", "check", "plan", "apply", "remove", "run", "schema"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lineup"))
            .args([command, "--help"])
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success());
        assert!(value["help"].as_str().unwrap().contains("Exit codes"));
    }
}

#[test]
fn published_schema_cannot_drift_from_rust_types() {
    let published: Value = serde_json::from_str(include_str!("../manifest.schema.json")).unwrap();
    assert_eq!(
        published,
        lineup::management::schema(),
        "regenerate with lineup schema"
    );
}
