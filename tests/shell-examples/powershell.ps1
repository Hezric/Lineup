#requires -Version 5.1
# Both PowerShell runtimes execute these documented examples.
$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$arguments = '"hello world" 中文'
if ($PSVersionTable.PSVersion.Major -lt 7) { $arguments = '\"hello world\" 中文' }
lineup.exe list | Set-Content -Encoding UTF8 list.json
lineup.exe check | Set-Content -Encoding UTF8 check.json
lineup.exe schema | Set-Content -Encoding UTF8 schema.json
lineup.exe --version | Set-Content -Encoding UTF8 version.json
lineup.exe --help | Set-Content -Encoding UTF8 help.json
lineup.exe list --help | Set-Content -Encoding UTF8 command-help.json
lineup.exe plan --manifest startup.toml | Set-Content -Encoding UTF8 file-toml.json
lineup.exe plan --manifest startup.json | Set-Content -Encoding UTF8 file-json.json
Get-Content -Raw -Encoding UTF8 startup.toml | lineup.exe plan --manifest - | Set-Content -Encoding UTF8 stdin-toml.json
@'
{"schemaVersion":1,"entries":[{"name":"外壳测试","path":"%SystemRoot%\\System32\\notepad.exe","arguments":"\"hello world\" 中文"}]}
'@ | lineup.exe plan --manifest - | Set-Content -Encoding UTF8 stdin-json.json
lineup.exe plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments $arguments | Set-Content -Encoding UTF8 single.json
lineup.exe plan --name example --path '%SystemRoot%\System32\notepad.exe' --delay 30 | Set-Content -Encoding UTF8 delay.json
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
