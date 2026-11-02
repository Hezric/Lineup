# Build cache

Run local Rust checks, tests and builds through `scripts/with-build-cache.ps1`:

```powershell
pwsh -NoProfile -File scripts/with-build-cache.ps1 cargo test --locked
```

Pass Tauri and other build commands through the same entry. It resolves Machine
`TEMP`, then User `TEMP` on Windows (`TMPDIR`, otherwise `/tmp` on other hosts),
and selects a separate cache for this checkout. The printed path is authoritative.
Only Cargo intermediate output moves there through `CARGO_BUILD_BUILD_DIR`;
final binaries, examples and packages retain their normal `target/` locations.
Use a current stable Rust toolchain and PowerShell 7.

Before each command, the entry removes its own previous cache when it exceeds
8 GiB or has not been used for seven days. The current command may exceed that
size until the next invocation. A lock serializes this checkout's wrapped commands;
a busy cache, ownership mismatch or reparse point stops the command without
pruning. Each checkout owns its cleanup; no machine-wide service is installed.
A dormant checkout is pruned when next used, not by a background timer.

Development builds retain line tables and disable incremental compilation;
test builds inherit those settings. Release optimization is unchanged.
Direct Cargo commands and explicit environment/CLI overrides bypass the managed
entry, so use it for local work. CI's disposable runners clean their workspaces
at job completion. Frontend `dist/` is overwritten by the next build, and release
packaging replaces the matching version's staging folder.

Verify pruning, held-file protection, child exit codes and Cargo output separation:

```powershell
pwsh -NoProfile -File scripts/test-build-cache.ps1
```
