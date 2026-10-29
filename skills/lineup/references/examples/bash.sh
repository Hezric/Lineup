#!/usr/bin/env bash
set -euo pipefail
lineup.exe list >list.json
lineup.exe check >check.json
lineup.exe schema >schema.json
lineup.exe --version >version.json
lineup.exe --help >help.json
lineup.exe list --help >command-help.json
lineup.exe plan --manifest startup.toml >file-toml.json
lineup.exe plan --manifest startup.json >file-json.json
lineup.exe plan --manifest - <<'EOF' >stdin-toml.json
schemaVersion = 1
[[entries]]
name = '外壳测试'
path = '%SystemRoot%\System32\notepad.exe'
arguments = '"hello world" 中文'
EOF
lineup.exe plan --manifest - <<'EOF' >stdin-json.json
{"schemaVersion":1,"entries":[{"name":"外壳测试","path":"%SystemRoot%\\System32\\notepad.exe","arguments":"\"hello world\" 中文"}]}
EOF
lineup.exe plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments '"hello world" 中文' >single.json
lineup.exe plan --name example --path '%SystemRoot%\System32\notepad.exe' --delay 30 >delay.json
MSYS_NO_PATHCONV=1 lineup.exe plan --name '外壳测试' --path '%SystemRoot%\System32\notepad.exe' --arguments '/flag "hello world" 中文' >slash.json
