---
name: lineup
description: Inspect, plan, and manage Windows startup entries with Lineup, including manifests, delayed tasks, and elevation. Use for Windows logon programs, not services or process supervision.
---

# Lineup

Use the installed `lineup` executable. Get field and command details from that executable: `lineup --help` and `lineup <command> --help` return JSON help; `lineup schema` prints the manifest schema. Before writing commands, read the bundled [shell reference](references/shells.md) for the active shell: PowerShell 7, Windows PowerShell 5.1, cmd, Git Bash, or WSL. All required guidance is inside this Skill or the installed tool; network access and a repository checkout are unnecessary.

## Inspect, plan, apply

1. Run `lineup list` and `lineup check`. Read inventory warnings, missing targets, and duplicates; exit 0 alone does not mean the inventory is complete or healthy.
2. Describe only the requested names in a UTF-8 TOML or JSON manifest. Use `lineup plan --manifest startup.toml`, or `lineup plan --name example --path '%SystemRoot%\System32\notepad.exe' --delay 30` in PowerShell 7. Inspect the desired entries, replaced sources, and elevation needs before changing startup.
3. When the requested changes are authorized, run `lineup apply --manifest startup.toml`, inspect every result, then read back with `lineup list` and `lineup check`. Exit 3 requires same-account elevation before retrying; exit 4 can mean partial changes, so inspect the report first. Lineup never requests UAC itself.

Omitted names remain untouched. `enabled = false` removes all same-name sources in that scope. `remove` also removes every same-name source in the selected scope; use an id from `list` for ambiguous names. `run` starts a program immediately and requires authorization for that launch. Removing startup entries does not stop running programs. Lineup has no undo history.

## Results and recovery

Every invocation writes one JSON document, including failures and help. Inspect
warnings and per-entry results as well as the process exit code. Keep raw Windows
arguments intact; check `plan.entries[].desired.arguments` before applying.

| Exit code | Meaning / next action |
| --- | --- |
| 0 | Success; still inspect inventory warnings and health findings |
| 1 | General failure; inspect the error before retrying |
| 2 | Invalid command, input, or manifest; correct it using help or schema |
| 3 | Elevation required; preflight made no changes; retry with same-account elevation |
| 4 | Partial failure; inspect each result and read back state before retrying |

Use a Windows executable path, including when invoking through WSL. Manifests
are UTF-8 TOML or JSON and may have a leading byte-order mark. Delay, priority,
elevation, or a working directory selects a scheduled task; ordinary entries
use Run values. Run commands have a 260-character limit; `delaySeconds: 0`
selects a task for a longer command. `run` uses the caller's token and ignores
the configured startup delay and task priority.
