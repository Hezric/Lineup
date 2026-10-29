# Shells and encoding

Lineup reads manifests as UTF-8, with or without a leading byte-order mark.
The examples below are read-only; inspect the returned
`plan.entries[].desired.arguments` to check the raw Windows argument
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

## Manifest files

Save these as UTF-8 `startup.toml` and `startup.json` in the working directory.
The examples plan changes only; they do not register or launch programs.

```toml
schemaVersion = 1
[[entries]]
name = '外壳测试'
path = '%SystemRoot%\System32\notepad.exe'
arguments = '"hello world" 中文'
```

```json
{"schemaVersion":1,"entries":[{"name":"外壳测试","path":"%SystemRoot%\\System32\\notepad.exe","arguments":"\"hello world\" 中文"}]}
```

```text
lineup plan --manifest startup.toml
lineup plan --manifest startup.json
```

## Complete read-only examples

The bundled [PowerShell examples](examples/powershell.ps1),
[cmd examples](examples/cmd.cmd), and [Bash examples](examples/bash.sh)
exercise list, check, schema, help, version, both manifest formats, stdin,
single-entry arguments, Unicode, and delayed planning. They expect the two
manifest files above in the working directory and write JSON results there.
The repository's CI runs these same files and asserts the results. The Bash
forms also work through WSL Windows interop when `lineup.exe` is on PATH;
WSL itself is not exercised by the Windows shell check.