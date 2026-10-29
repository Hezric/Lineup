@echo off
chcp 65001 >nul
lineup.exe list >list.json
lineup.exe check >check.json
lineup.exe schema >schema.json
lineup.exe --version >version.json
lineup.exe --help >help.json
lineup.exe list --help >command-help.json
lineup.exe plan --manifest startup.toml >file-toml.json
lineup.exe plan --manifest startup.json >file-json.json
type startup.toml | lineup.exe plan --manifest - >stdin-toml.json
type startup.json | lineup.exe plan --manifest - >stdin-json.json
lineup.exe plan --name "外壳测试" --path "%SystemRoot%\System32\notepad.exe" --arguments "\"hello world\" 中文" >single.json
lineup.exe plan --name example --path "%SystemRoot%\System32\notepad.exe" --delay 30 >delay.json
exit /b %errorlevel%
