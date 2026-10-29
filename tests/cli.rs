use serde_json::Value;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

#[test]
fn stdin_toml_uses_manifest_validation() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lineup"))
        .args(["plan", "--manifest", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"schemaVersion = 1\n[[entries]]\nname = 'bad/name'\npath = 'C:\\app.exe'\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(value["problems"][0]["path"], "/entries/0/name");
}

fn input(args: &[&str], raw: &str) -> (i32, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lineup"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(raw.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap(),
    )
}

#[test]
fn manifest_formats_and_argument_validation() {
    for raw in [
        " # comment\nschemaVersion = 1\n[[entries]]\nname = 'example'\npath = 'C:\\app.exe'\n",
        " \n{\"schemaVersion\":1,\"entries\":[{\"name\":\"example\",\"path\":\"C:\\\\app.exe\"}]}",
    ] {
        let (code, value) = input(&["plan", "--manifest", "-"], raw);
        assert_ne!(code, 2, "{value}");
    }
    let (code, value) = input(
        &["plan", "--manifest", "-"],
        "schemaVersion = 1\n[[entries]\n",
    );
    assert_eq!(code, 2);
    let reason = value["problems"][0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("line 2") && reason.contains("column"),
        "{reason}"
    );
    let (code, _) = input(&["plan", "--manifest", "-", "--name", "x"], "{}");
    assert_eq!(code, 2);
    let (code, value) = input(
        &[
            "plan",
            "--name",
            "bad/name",
            "--path",
            "C:\\app.exe",
            "--delay",
            "30",
        ],
        "",
    );
    assert_eq!(code, 2);
    assert_eq!(value["problems"][0]["path"], "/entries/0/name");
    let (code, value) = input(&["plan", "--name", "example", "--path", "C:\\app.exe"], "");
    assert_ne!(code, 2, "{value}");
    if code == 0 {
        let desired = &value["plan"]["entries"][0]["desired"];
        assert_eq!(desired["scope"], "user");
        assert_eq!(desired["arguments"], "");
        assert_eq!(desired["enabled"], true);
        assert_eq!(desired["elevated"], false);
    }
}

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
