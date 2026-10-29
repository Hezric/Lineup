---
name: lineup
description: Inspect, plan, and manage Windows startup entries with Lineup, including manifests, delayed tasks, and elevation. Use for Windows logon programs, not services or process supervision.
---

# Lineup

Use the installed `lineup` executable. Read the [usage guide](https://github.com/Hezric/Lineup/blob/main/docs/usage.md) for manifest fields, shell examples, and exit codes; `lineup --help` and `lineup <command> --help` return JSON help. `lineup schema` prints the manifest schema.

## Inspect, plan, apply

1. Run `lineup list` and `lineup check`. Read inventory warnings, missing targets, and duplicates; exit 0 alone does not mean the inventory is complete or healthy.
2. Describe only the requested names in a UTF-8 TOML or JSON manifest. Use `lineup plan --manifest startup.toml`, or `lineup plan --name example --path '%SystemRoot%\System32\notepad.exe' --delay 30` in PowerShell 7. Inspect the desired entries, replaced sources, and elevation needs before changing startup.
3. When the requested changes are authorized, run `lineup apply --manifest startup.toml`, inspect every result, then read back with `lineup list` and `lineup check`. Exit 3 requires same-account elevation before retrying; exit 4 can mean partial changes, so inspect the report first. Lineup never requests UAC itself.

Omitted names remain untouched. `enabled = false` removes all same-name sources in that scope. `remove` also removes every same-name source in the selected scope; use an id from `list` for ambiguous names. `run` starts a program immediately and requires authorization for that launch. Removing startup entries does not stop running programs. Lineup has no undo history.

## Shell boundaries

Every invocation writes one JSON document, including failures and help. Keep raw Windows arguments intact and inspect `plan.entries[].desired.arguments` before applying.

- In PowerShell 7, single-quote Windows paths and raw arguments. Windows PowerShell 5.1 uses legacy native quoting: escape embedded double quotes with backslashes. Set `$OutputEncoding = [System.Text.UTF8Encoding]::new($false)` before piping non-ASCII manifests; use `Get-Content -Raw -Encoding UTF8 startup.toml | lineup plan --manifest -`.
- In cmd, use `chcp 65001` for non-ASCII arguments and `type startup.toml | lineup plan --manifest -` for UTF-8 file bytes. Account for `%VARIABLE%` expansion.
- In Git Bash, use `lineup.exe`, single-quoted Windows paths, and quoted heredocs for stdin. Prefix a command with `MSYS_NO_PATHCONV=1` when a raw application argument begins with `/`. In WSL, call the Windows executable with Windows paths.

Use the guide's examples for the active shell rather than translating quoting by eye. The Skill carries workflow and pitfalls; the guide and CLI own command details.
