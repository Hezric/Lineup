use serde_json::{Value, json};

const HELP: &str = "Lineup: JSON-first Windows startup manager\n\
Usage: lineup <command> [options]\n\
Commands:\n\
  list                       Read both scopes: Run (including 32-bit), Startup\n\
                             folders, and logon/boot tasks. See inventory.warnings.\n\
  check                      Report missing executable targets and duplicate paths.\n\
  plan --manifest <file>      Validate JSON and preview exact managed-entry actions.\n\
  apply --manifest <file>     Validate, preflight, and apply the same actions.\n\
  remove <name|id>           Remove every same-name source within that scope.\n\
  run <name|id>              Launch now with arguments and working directory;\n\
                             report pid. Does not wait or forward child output.\n\
  schema                     Print the generated manifest JSON Schema.\n\
  --help, -h                 Help (also available after every command).\n\
  --version, -V              Version.\n\
Manifest: {\"schemaVersion\":1,\"entries\":[{\"name\":\"example\",\n\
\"path\":\"C:\\\\Windows\\\\notepad.exe\"}]}\n\
Entry fields: name (unique within scope, 1..200 characters, no path separators,\n\
reserved Windows filename characters, control characters, trailing dot/space);\n\
path (absolute Windows executable path; %VARIABLE% expansion supported);\n\
arguments (optional raw Windows command line, default empty); workingDirectory\n\
(optional absolute path); scope (user default, or machine); elevated (false\n\
default); delaySeconds (optional unsigned seconds after logon); priority\n\
(optional Task Scheduler priority 0..10, default 7 for tasks; lower is higher);\n\
enabled (true default). Unknown fields and all invalid fields are rejected.\n\
Plain entries use Run; delay, priority, elevation, or workingDirectory use a\n\
logon task under \\Lineup\\. enabled:false removes all same-name sources in scope.\n\
Only manifest names or explicitly selected names are changed. Ambiguous names\n\
need an id from list. list/check/plan never write. No automatic UAC or self-start.\n\
Every invocation writes one JSON document, including help/errors.\n\
Exit codes: 0 success; 1 general failure; 2 invalid input/manifest;\n\
3 elevation required (preflight made no changes); 4 partial failure.\n\
Warnings carry scope/name/message; only uncertainty affecting managed names blocks changes. apply with\n\
any required elevation must be called elevated by the same user. Removal never\n\
terminates running programs. Lineup never reboots or signs out.";

fn failure(error: impl std::fmt::Display, code: i32) -> (Value, i32) {
    (json!({"ok": false, "error": error.to_string()}), code)
}

fn execute(args: &[String]) -> (Value, i32) {
    if args == ["--version"] || args == ["-V"] {
        return (json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}), 0);
    }
    let commands = ["list", "check", "plan", "apply", "remove", "run", "schema"];
    if args.is_empty()
        || args == ["--help"]
        || args == ["-h"]
        || (args.len() == 2
            && commands.contains(&args[0].as_str())
            && ["--help", "-h"].contains(&args[1].as_str()))
    {
        return (json!({"ok": true, "help": HELP}), 0);
    }
    if args == ["schema"] {
        return (lineup::management::schema(), 0);
    }
    let Some(command) = args.first().map(String::as_str) else {
        unreachable!()
    };
    let valid = match command {
        "list" | "check" => args.len() == 1,
        "plan" | "apply" => args.len() == 3 && args[1] == "--manifest",
        "remove" | "run" => args.len() == 2,
        _ => false,
    };
    if !valid {
        return (
            json!({"ok": false, "error": "invalid command or options", "help": HELP}),
            2,
        );
    }
    let manifest = if ["plan", "apply"].contains(&command) {
        let raw = match std::fs::read_to_string(&args[2]) {
            Ok(raw) => raw,
            Err(e) => return failure(e, 2),
        };
        match lineup::management::parse_manifest(&raw) {
            Ok(manifest) => Some(manifest),
            Err(problems) => return (json!({"ok": false, "problems": problems}), 2),
        }
    } else {
        None
    };
    #[cfg(windows)]
    {
        use lineup::{Backend, management};
        let mut backend = match lineup::native::WindowsBackend::new() {
            Ok(v) => v,
            Err(e) => return failure(e, 1),
        };
        let result = match command {
            "list" => backend
                .inventory()
                .map(|v| (json!({"ok": true, "inventory": v}), 0)),
            "check" => lineup::check(&backend).map(|v| (json!({"ok": true, "health": v}), 0)),
            "plan" => management::plan(&backend, manifest.as_ref().expect("parsed manifest"))
                .map(|v| (json!({"ok": true, "plan": v}), 0)),
            "apply" | "remove" => {
                let result = if command == "apply" {
                    management::apply(&mut backend, manifest.as_ref().expect("parsed manifest"))
                } else {
                    management::remove(&mut backend, &args[1])
                };
                result.map(|report| {
                    let code = match report.status.as_str() {
                        "elevation_required" => 3,
                        "partial_failure" => 4,
                        _ => 0,
                    };
                    (json!({"ok": code == 0, "report": report}), code)
                })
            }
            "run" => management::run(&backend, &args[1]).map(|v| {
                if v["status"] == "elevation_required" {
                    (json!({"ok": false, "report": v}), 3)
                } else {
                    (json!({"ok": true, "started": v}), 0)
                }
            }),
            _ => unreachable!(),
        };
        result.unwrap_or_else(|e| failure(e, 1))
    }
    #[cfg(not(windows))]
    {
        let _ = manifest;
        failure("Windows is required", 1)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (output, code) = execute(&args);
    println!("{output}");
    std::process::exit(code);
}
