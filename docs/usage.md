# Lineup

Lineup manages the programs Windows starts at logon. It is built for coding agents and scripts: describe the startup entries you want in a TOML or JSON manifest, preview the changes with `lineup plan`, apply them with `lineup apply`, and read every result as JSON.

## Installation

For Scoop and release downloads, see the [installation instructions](../README.md#install).
To build from source on Windows 11 x64, install the MSVC build tools and Rust
through rustup, then use the repository's pinned stable toolchain:

```powershell
cargo build --release --locked
```

Copy `target/release/lineup.exe` to a directory on your PATH. Lineup never
registers itself for startup or stays resident.

## Commands

```powershell
lineup list
lineup check
lineup plan --manifest startup.toml
lineup apply --manifest startup.toml
lineup plan --name example --path '%SystemRoot%\System32\notepad.exe' --delay 30
Get-Content -Raw startup.toml | lineup plan --manifest -
lineup remove example
lineup run example
lineup schema
lineup --help
lineup list --help
lineup --version
```

`list` returns `inventory.entries` and `inventory.warnings`. It reads current-user
and machine Run values (including the 32-bit machine view), Startup folders,
and scheduled tasks with logon or boot triggers. Microsoft system task folders,
services, and other users' tasks are excluded. `check` returns the inventory,
missing target ids, and duplicate executable groups under `health`.

Each entry includes a stable id, name, scope, source, locator, path, raw Windows
arguments, working directory, enabled state, elevation, delay, priority, and boot
trigger state. Enabled state includes Task Manager's `StartupApproved` records.
Inventory entries expose the raw leading DWORD as `startupApprovedState` (decimal, or
`null` when absent/unreadable). `enabled` is `null` when an approval record is
unknown, malformed, or unreadable; Lineup does not guess its status. Known
states are `2`, `4`, `6`, `8` for enabled and `1`, `3`, `5`, `7`, `9` for disabled.
Other states, including `0`, produce a warning. See the Feature document's
[source notes](features/startup-management.md#startupapproved-evidence).
Warnings are objects with `scope`, `name`, and `message`; a null scope or name
means the failure could affect either scope or any name, respectively.
Neither command changes Windows or requires administrator rights. Lineup-created
machine tasks and their machine folder grant authenticated users read access;
only Administrators and SYSTEM can modify them. The policy is reapplied on task
updates, and task registration does not add broader rights for the principal.

Task Scheduler can omit unreadable objects from its collections. Lineup warns
on detected task/folder read failures, checks task-store names when that index
is readable, and probes its known machine folder directly. Planning also probes
the expected managed task path before treating an absent entry as a new one;
access denial fails the plan instead of reporting `create`. Objects hidden by
both the Scheduler API and inaccessible indexes cannot be named by an ordinary
process. Use an elevated inventory for a complete view of such private tasks.

Tasks created before this permission policy need an elevated update or
recreation to acquire it. An unchanged apply does not rewrite existing ACLs.

## Manifest and changes

Save this as `startup.toml`:

```toml
schemaVersion = 1

[[entries]]
name = 'example'
path = '%SystemRoot%\System32\notepad.exe' # Windows path
delaySeconds = 30
priority = 7
```

JSON is also accepted. Content beginning with `{` after whitespace is JSON;
other content is TOML. TOML syntax errors include line and column. `--manifest -`
reads stdin. Alternatively, `plan` and `apply` accept `--name`, `--path`,
`--arguments`, `--working-directory`, `--scope`, `--elevated`, `--delay`,
`--priority`, and `--enabled` for one entry, mutually exclusive with `--manifest`.
Boolean options accept `true` or `false`; a bare flag means `true`.
The same defaults and validation apply to all three input forms.

### Shells and encoding

Lineup reads manifests as UTF-8. The examples below are read-only; inspect the
returned `plan.entries[].desired.arguments` to check the raw Windows argument
string. Use UTF-8 when saving manifest files.

**PowerShell 7:**

```powershell
lineup plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments '"hello world" 中文'
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
@'
schemaVersion = 1
[[entries]]
name = '外壳测试'
path = '%SystemRoot%\System32\notepad.exe'
arguments = '"hello world" 中文'
'@ | lineup plan --manifest -
@'
{"schemaVersion":1,"entries":[{"name":"外壳测试","path":"%SystemRoot%\\System32\\notepad.exe","arguments":"\"hello world\" 中文"}]}
'@ | lineup plan --manifest -
```

**Windows PowerShell 5.1:** Use the same here-string pipes and UTF-8
`$OutputEncoding` assignment above, but escape embedded double quotes for native
arguments with backslashes:

```powershell
lineup plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments '\"hello world\" 中文'
Get-Content -Raw -Encoding UTF8 startup.toml | lineup plan --manifest -
```

PowerShell 5.1 otherwise commonly pipes using an ASCII encoding; characters can
be lost before Lineup receives them. The PowerShell 7 argument example assumes
the default native argument passing mode; legacy mode follows the 5.1 form.

**cmd:** In an interactive UTF-8 console, use `chcp 65001` for non-ASCII
arguments, and save `startup.toml` and `startup.json` as UTF-8. `type` passes
their bytes through unchanged. `%SystemRoot%` is expanded by cmd; in a batch
file use `%%SystemRoot%%` instead.

```cmd
chcp 65001
lineup plan --name "外壳测试" --path "%SystemRoot%\System32\notepad.exe" --arguments "\"hello world\" 中文"
type startup.toml | lineup plan --manifest -
type startup.json | lineup plan --manifest -
```

**Git Bash and WSL:** Quote Windows paths and use a quoted heredoc delimiter to
prevent shell expansion. In WSL, call the Windows executable as `lineup.exe`;
pass a Windows path such as the one below, not `/mnt/c/...`.

```bash
lineup.exe plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments '"hello world" 中文'
lineup.exe plan --manifest - <<'EOF'
schemaVersion = 1
[[entries]]
name = '外壳测试'
path = '%SystemRoot%\System32\notepad.exe'
arguments = '"hello world" 中文'
EOF
lineup.exe plan --manifest - <<'EOF'
{"schemaVersion":1,"entries":[{"name":"外壳测试","path":"%SystemRoot%\\System32\\notepad.exe","arguments":"\"hello world\" 中文"}]}
EOF
```

Git Bash may turn a native argument beginning with `/` into a Windows path.
Disable conversion for that invocation when the slash is an application flag:

```bash
MSYS_NO_PATHCONV=1 lineup.exe plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments '/flag "hello world" 中文'
```

The [JSON Schema](../manifest.schema.json) is generated from the Rust types and
checked by tests. Regenerate it with `lineup schema`. Unknown fields, invalid
types, ranges, names, and duplicate names within a scope are reported together
with JSON Pointer field paths before any change. Names are case-insensitive
within a scope, contain 1–200 characters, and cannot contain reserved Windows
filename characters, control characters, or trailing dots/spaces. Paths must
be absolute Windows paths; environment variables such as `%SystemRoot%` expand.

| Entry field | Default / meaning |
| --- | --- |
| `name` | Required; identity within a scope |
| `path` | Required; executable path |
| `arguments` | Empty; raw Windows arguments, preserving quoting |
| `workingDirectory` | Unspecified; absolute working directory |
| `scope` | `user`; `machine` selects all users |
| `elevated` | `false`; highest-privilege scheduled task |
| `delaySeconds` | Unspecified; unsigned seconds after logon |
| `priority` | Unspecified; Task Scheduler priority 0–10 (default 7 for tasks) |
| `enabled` | `true`; `false` removes every same-name source in the scope |

`plan` is read-only and reports create, update, enable, disable, or unchanged,
including the desired source and each replaced realization. `apply` executes
that plan, leaving names outside the manifest untouched. Applying the same
manifest again reports unchanged entries. Omitting a name from a later manifest
does not remove it. To remove it, set `enabled` to `false` or use `remove`.

Plain entries use a Run value. Delay, priority, elevation, or a working directory
require a logon task: Run values cannot store a working directory. Tasks retain
the entry name under `\Lineup\User-<sid>\` or `\Lineup\Machine\`; the subfolders
keep users and scopes from overwriting each other. Task priority 0 is highest
and 10 lowest. Run commands have Windows' 260-character limit; specify
`delaySeconds: 0` to use a task for a longer command.

Replacement registers the desired source before deleting obsolete sources.
Failures are reported per entry and other entries continue; inspect the report
and rerun after resolving errors. An entry may have been partially changed
before a failure. Lineup does not keep undo history.

`remove <name|id>` removes all same-name sources in the selected scope and
reports every removed realization. Names spanning scopes require an id from
`list`. `run <name|id>` launches one realization immediately, with its arguments
and working directory, and reports its process id. Names spanning multiple
sources require an id. Child output is not forwarded. Neither command terminates
running programs; removing a task leaves its running program alone.

Inventory warnings block `plan`/`apply` only when uncertainty could affect a
managed name in its scope, including same-name realizations in other sources.
An unreadable source or folder blocks managed names in that scope; an unknown
scope blocks either scope. Unknown approval state on an unrelated name or the
other scope does not block. All warnings remain in `plan.warnings` and
`report.plan.warnings`, including when elevation is required.

`remove`/`run` apply the same safety rule to the selected name. Name selection
also checks uncertainty in the other scope because it could hide an ambiguous
match; an exact id fixes the scope. Their successful results retain warnings
in `report.plan.warnings` or `started.warnings`, respectively.

## Scopes and elevation

Current-user entries belong to the user running Lineup. Machine entries apply
to all users and require an administrator token to change. Elevated entries
use highest-privilege logon tasks and also require an administrator token to
register, replace, or remove. Lineup checks the entire plan before writing: if
any action needs elevation, an unelevated `apply` returns exit 3 with the full
plan and performs no actions. Unchanged privileged entries do not require
elevation. `remove` uses the same preflight.

Elevate the whole command yourself, using the same Windows account (for example
`gsudo lineup apply --manifest startup.toml`). Lineup never requests UAC itself.
Successfully registered elevated user tasks subsequently start at that user's
logon without another prompt. Machine tasks use the built-in Users group, or
Administrators for elevated tasks, with a logon trigger for eligible users.
An elevated machine task runs at administrator logons; standard accounts cannot
obtain administrator privileges without credentials. No credentials are stored.

`run` uses the caller's token, ignores the configured startup delay, and requires
elevation for an elevated entry. It does not automatically apply a task's
scheduler priority to a direct manual launch. Use an ordinary token for an
ordinary manual launch, as with other command-line programs.

Every invocation writes exactly one JSON document to stdout, including help and
failures. Health findings and inventory warnings do not make a read-only command
fail; inspect the result as well as the exit code.

| Exit code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | General failure |
| 2 | Invalid command, input, or manifest |
| 3 | Elevation required; preflight made no changes |
| 4 | Partial failure; inspect per-entry results |

## Development

```powershell
cargo fmt --check
cargo clippy --all-targets -j 2 -- -D warnings
cargo test -j 2
$env:LINEUP_REAL_TESTS = '1'
cargo test -j 2 --test real_windows -- --nocapture
Remove-Item Env:LINEUP_REAL_TESTS
```

The core library separates operations from native Windows access through a
backend interface. [Current behavior](features/startup-management.md) describes
the delivered contract.

The opt-in suite creates only unique current-user `LineupTest-` entries and
cleans them up even when assertions fail. It covers Run values, Startup shortcut
reads, delayed tasks, priority, working directory, idempotence, and `run`.
The current-user suite also runs in an already-elevated session, as on CI. Its
ordinary-token refusal check runs when called without elevation; unit tests and
the elevated suite's linked-token child cover that boundary on an elevated runner.

For privileged verification, use a disposable Windows runner or a supervised
administrator session, with the same Windows account:

```powershell
$env:LINEUP_ELEVATED_TESTS = '1'
cargo test -j 2 --test real_windows_elevated -- --nocapture
Remove-Item Env:LINEUP_ELEVATED_TESTS
```

This suite refuses an ordinary token before making changes. It uses only unique
`LineupTest-` names and cleans up created entries, owned task instances, and
folders on success or failure; any pre-existing machine-folder ACL is restored.
It reads back a machine Run value, a delayed machine task, an elevated user
task, and a delayed elevated machine task. It checks task triggers, delay,
identity, privilege level, command, arguments, working directory, and machine
ACLs before and after updates. Each task runs a test-owned probe on demand to
verify actual process elevation. A child using the account's linked ordinary token verifies read access,
unchanged planning, denied machine-task modification, and elevation preflight.
Without a split token (for example when UAC is disabled), Windows ignores LUA
run levels: the suite expects the caller's elevation and reports the ordinary-token
runtime checks unavailable. ACL decoding and requested run-level checks still run.
The two ignored helper probes are started only by this suite.

[Windows CI](../.github/workflows/ci.yml) runs fmt, clippy, unit/CLI tests,
and both integration suites on push and pull request. Rustup installs the
version selected by `rust-toolchain.toml`. CI uses an already-elevated Windows
runner; it does not request UAC, sign out, reboot, or change Windows settings.
Lineup verifies the definitions it writes and on-demand execution; actual logon
trigger delivery is Windows behavior and is outside end-to-end testing.
