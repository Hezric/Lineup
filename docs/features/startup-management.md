# Startup management

Lineup is a Windows x64 executable with a reusable Rust library. `list` reads
current-user and machine Run registry values, Startup-folder files and shell
links, and executable actions in scheduled tasks with logon or boot triggers.
The shared HKCU Run view is listed once; both HKLM registry views are read.
Tasks under `\Microsoft\` and tasks belonging to another user are excluded.
Services are outside the inventory.

Entries expose a stable source/scope/locator id, name, executable, raw Windows
arguments, working directory, enabled state, elevation, delay, priority, and
whether a boot trigger exists. Tasks with multiple actions have action-index
suffixes on their ids. Non-executable task actions and unreadable sources appear
in warnings rather than disappearing silently.

Scheduled-task executable paths have surrounding double quotes removed and
expand environment variables. Relative commands resolve against the configured
working directory first, then PATH. Commands without an extension try PATHEXT
extensions in order. Inventory reports the resolved file path when found;
`check` reports a missing target only when resolution finds no file.

Run and Startup-folder enabled states come from Explorer's `StartupApproved`
records. The raw leading DWORD appears as `startupApprovedState` (decimal or
null when absent/unreadable). Known states `2`, `4`, `6`, `8` mean enabled;
`1`, `3`, `5`, `7`, `9` mean disabled. Unknown states, including `0`, and malformed
or unreadable records produce `enabled: null` and a warning. Missing approval
records leave the entry enabled. Task entries keep their native enabled state
and have no approval DWORD. `check` reports missing
executable targets and groups case-insensitive executable paths appearing more
than once. It does not claim that arguments or DLL payloads are equivalent.
Both commands are read-only and do not need administrator rights. Detected task
or folder access errors appear in warnings, including individual collection
item failures. Enumeration cross-checks task-store names when accessible and
probes the known machine folder even when the folder collection omits it.
Scheduler collections can hide unreadable objects without errors; an ordinary
process cannot identify objects also hidden by inaccessible indexes.

Every invocation, including help and errors, writes one JSON document. See the
[README](../../README.md) for commands and exit codes. Verification uses library
tests and read-only real-machine `list` and `check` smoke checks.

## Desired state and single entries

The [manifest schema](../../manifest.schema.json) is derived from Rust types,
published for editors, and compared against generated output by a test. Runtime
validation reports every detected field problem and rejects duplicate names
within a scope before changes.
`plan` and `apply` accept TOML or JSON manifests from a file or stdin (`--manifest -`),
or one entry via CLI options. JSON is detected by its first non-whitespace `{`;
all other input is TOML. One leading UTF-8 byte-order mark is accepted before
format detection, including stdin from Windows PowerShell. Syntax errors in TOML
include line and column, and field errors use JSON Pointer paths. All input
forms share defaults and validation.
For example:

```toml
schemaVersion = 1
[[entries]]
name = 'example'
path = '%SystemRoot%\System32\notepad.exe' # Windows path
```

Use `lineup plan --name example --path '%SystemRoot%\System32\notepad.exe'`
for a single entry, or `Get-Content -Raw startup.toml | lineup plan --manifest -`
to pipe a manifest.
The library plans and applies through `Backend`. Only named entries in a
manifest are managed. Plain entries use Run values; delay, priority, elevation,
or working directory select a logon task. Run cannot store a working directory.
Tasks are named after the entry beneath scope/user subfolders of `\Lineup\` to
avoid collisions. Disabled entries are absent from every source in their scope.
Matching is case-insensitive by name and scope; replacing an entry converges
all its sources to one realization. Missing manifest names are left alone.

When a managed name is absent from inventory, planning directly checks its
expected task locator before claiming absence. A denied lookup or an existing
but unenumerated task fails planning rather than becoming create or unchanged.

Plans include actions, desired configuration, obsolete realizations, and
elevation needs. Apply writes the new realization before removing old sources,
clears obsolete `StartupApproved` disabled flags, and reports each entry as
applied, unchanged, or failed. Failures do not prevent independent entries from
being attempted. Warnings carry `scope`, `name`, and `message` at the inventory
read boundary. A null scope or name means uncertainty in either scope or over
any name. Planning blocks only warnings that can affect a managed name in the
same scope, across all sources; unrelated warnings remain in `plan.warnings`
and `report.plan.warnings`. Source/folder read failures block the affected
scope, and uncertainty whose scope is unknown blocks either scope.

`remove` deletes all same-name sources within the selected scope, including when
selected by an id. Ambiguous names across scopes require an id. `run` starts a
single selected executable with raw Windows arguments and its working directory,
without waiting or forwarding child output, and returns the process id. Ambiguous
names across sources require an id. Both commands check uncertainty affecting
the selected name and preserve unrelated warnings. Name selection also checks
both scopes to rule out ambiguity; an id narrows safety checks to its scope.
Run results expose `started.warnings`; remove exposes `report.plan.warnings`.
Lineup never terminates existing processes.

Verification includes the memory-backend operation tests, CLI JSON/help and
schema tests, and `LINEUP_REAL_TESTS=1 cargo test --test real_windows -j 2`.
The opt-in suite uses unique current-user `LineupTest-` entries with failure
cleanup, then compares the full inventory before and after.

## Privileged actions

Planning marks every changing machine-scope or elevated entry, including
replacement or deletion of an existing elevated realization. Apply and remove
preflight the entire plan against the caller's token before any write. An
unelevated caller receives the full plan, status `elevation_required`, no entry
results, and exit 3. A second unchanged apply does not require elevation.

The native backend uses the effective token's SID so same-account elevation
retains current-user scope. User tasks use interactive-token logon for that
identity; elevated tasks use highest privileges. Machine tasks use group logon
for built-in Users, or Administrators when elevated, and no user-specific
trigger. Elevated machine tasks therefore apply to administrator logons.
Standard users cannot be silently granted administrator credentials.

Machine task registrations and their machine folder use a protected ACL owned
by Administrators: SYSTEM and Administrators have full control; authenticated
users have read access only. The same descriptor is supplied on every update,
and automatic principal ACE creation is disabled for machine tasks. User-task
permissions retain Windows defaults. Older registrations acquire the policy on
an elevated update or recreation; unchanged entries are not rewritten.

Manual `run` uses the caller's token and returns exit 3 for an elevated entry
under an ordinary token. It starts immediately without the logon delay; scheduler
priority applies to scheduled starts. The library never elevates itself.

Memory tests cover mixed-manifest refusal with zero writes, privileged retry,
same-name entries in separate scopes, disable/remove preflight, and idempotence.
Native tests inspect unregistered COM definitions and decode the actual Windows
ACL rights. The current-user suite uses `LINEUP_REAL_TESTS=1` and works with an
ordinary or elevated token. `LINEUP_ELEVATED_TESTS=1` enables a separate suite
that requires elevation before writing. It verifies all four entry kinds by
native definition readback, task execution with elevation probes, update ACL
preservation, and idempotence. Its linked-token child checks ordinary-user read
access, denied writes, and elevation preflight when the account has a split token.
Without one, Windows ignores LUA run levels; runtime expectations follow the
caller's elevation, ordinary-token checks are reported unavailable, and definition
and ACL checks still run. Cleanup stops only owned task
instances, deletes only recorded test entries/folders, and restores a
pre-existing machine-folder ACL.

The [Windows workflow](../../.github/workflows/ci.yml) runs fmt, clippy,
unit tests, the [read-only shell examples](../../tests/shell-examples.ps1),
and both opt-in suites on push and pull request using the pinned
toolchain. Real logon delivery is delegated to Windows and is not tested by
signing out or rebooting. The earlier supervised readback/on-demand verification
passed for all four kinds; new ACL behavior requires the elevated suite or CI
run, rather than being inferred from those earlier results.

The [companion Skill](../../skills/lineup/SKILL.md) provides an agent workflow
and bundled shell references/examples that work without network access or a repository checkout. Field-level details come from installed CLI help and schema output.
It is discoverable by the skills CLI and included in release archives. The
README's installation prompt respects the user's existing Skill management.

## StartupApproved evidence

The registry blob is an observed Explorer format, not a published Microsoft
binary contract. The [Microsoft StartupTaskState API](https://learn.microsoft.com/en-us/uwp/api/windows.applicationmodel.startuptaskstate)
is a different interface; its values and policy labels are not used to decode
this registry DWORD.

- [Nutanix's Windows startup testing](https://www.nutanix.com/en_sg/blog/windows-os-optimization-essentials-part-4-startup-items)
  explicitly reports `02`/`04` enabled and `03`/`05` disabled.
- [Firsthand Task Manager/registry experiments](https://superuser.com/questions/1766797/how-entries-at-run-registry-are-overridden-by-windows-settings)
  describe `01` disabled, `08` enabled and `09` disabled. The linked
  [registry observations](https://renenyffenegger.ch/notes/Windows/registry/tree/HKEY_CURRENT_USER/Software/Microsoft/Windows/CurrentVersion/Explorer/StartupApproved/Run/index)
  identify `06` as enabled; observed even/odd behavior also covers `07` disabled.
- [Windows Incident Response's controlled tests](https://windowsir.blogspot.com/2022/07/startupapprovedrun-pt-ii.html)
  distinguish absent records from explicitly recorded states. Absence means
  enabled; an explicit unverified value such as `0` is not treated as absence.

Lineup recognizes only the listed values and does not extrapolate parity to
new values or assign unsupported policy meanings. Backend-seam tests cover
known states, unknown raw DWORDs (including nonzero upper bytes), truncated
records, and matching uncertainty by scope/name for plan, apply, remove and run.
