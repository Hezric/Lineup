#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target/release/lineup.exe'
if (!(Test-Path -LiteralPath $binary)) { throw 'Build the release executable first.' }
$gitExec = (& git --exec-path).Trim()
$bash = [IO.Path]::GetFullPath((Join-Path $gitExec '../../../bin/bash.exe'))
if (!(Test-Path -LiteralPath $bash)) { throw 'Git Bash is required.' }
$temp = [Environment]::GetEnvironmentVariable('TEMP', 'Machine')
if (!$temp) { $temp = [Environment]::GetEnvironmentVariable('TEMP', 'User') }
$root = Join-Path $temp ('aiconf-lineup-shells-' + [guid]::NewGuid().ToString('N'))
$originalPath = $env:PATH
$originalDirectory = Get-Location
$utf8 = [Text.UTF8Encoding]::new($false)
$fixture = @'
schemaVersion = 1
[[entries]]
name = '外壳测试'
path = '%SystemRoot%\System32\notepad.exe'
arguments = '"hello world" 中文'
'@
$json = '{"schemaVersion":1,"entries":[{"name":"外壳测试","path":"%SystemRoot%\\System32\\notepad.exe","arguments":"\"hello world\" 中文"}]}'
function Read-Result($name) {
    return [IO.File]::ReadAllText((Join-Path (Get-Location) "$name.json")) | ConvertFrom-Json
}
try {
    $env:PATH = (Split-Path $binary -Parent) + [IO.Path]::PathSeparator + $originalPath
    foreach ($shell in @('pwsh', 'powershell', 'cmd', 'bash')) {
        $working = Join-Path $root $shell
        New-Item -ItemType Directory -Path $working -Force | Out-Null
        [IO.File]::WriteAllText((Join-Path $working 'startup.toml'), $fixture, $utf8)
        [IO.File]::WriteAllText((Join-Path $working 'startup.json'), $json, $utf8)
        Set-Location -LiteralPath $working
        switch ($shell) {
            'pwsh' { & pwsh -NoProfile -File (Join-Path $PSScriptRoot '../skills/lineup/references/examples/powershell.ps1') }
            'powershell' {
                # EncodedCommand lets 5.1 read the UTF-8 source without a BOM or ANSI loss.
                $source = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '../skills/lineup/references/examples/powershell.ps1'))
                & powershell.exe -NoProfile -EncodedCommand ([Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($source)))
            }
            'cmd' {
                # cmd's code-page switch needs CRLF batch input; repository text uses LF.
                $batch = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '../skills/lineup/references/examples/cmd.cmd')).Replace("`r`n", "`n").Replace("`n", "`r`n")
                [IO.File]::WriteAllText((Join-Path $working 'examples.cmd'), $batch, $utf8)
                & cmd.exe /d /c (Join-Path $working 'examples.cmd')
            }
            'bash' { & $bash (Join-Path $PSScriptRoot '../skills/lineup/references/examples/bash.sh') }
        }
        if ($LASTEXITCODE -ne 0) { throw "$shell failed: $LASTEXITCODE" }
        foreach ($name in @('file-toml', 'file-json', 'stdin-toml', 'stdin-json', 'single', 'delay')) {
            $result = Read-Result $name
            if (!$result.ok) { throw "$shell/${name}: $($result | ConvertTo-Json -Depth 12 -Compress)" }
            $desired = $result.plan.entries[0].desired
            if (!$result.ok -or $result.plan.entries.Count -ne 1 -or $desired.scope -ne 'user' -or
                $desired.path -ine (Join-Path $env:SystemRoot 'System32/notepad.exe')) { throw "$shell/${name}: invalid plan" }
            if ($name -eq 'delay') {
                if ($desired.name -cne 'example' -or $desired.delaySeconds -ne 30 -or $desired.source -ne 'task') { throw "$shell/delay mismatch" }
            } elseif ($desired.name -cne '外壳测试' -or $desired.arguments -cne '"hello world" 中文' -or $desired.source -ne 'run') {
                throw "$shell/$name changed Unicode or raw arguments"
            }
        }
        $inventory = Read-Result 'list'
        $health = Read-Result 'check'
        $schema = Read-Result 'schema'
        if (!$inventory.ok -or $null -eq $inventory.inventory.entries -or $null -eq $inventory.inventory.warnings) { throw "$shell/list invalid" }
        if (!$health.ok -or $null -eq $health.health.missingTargets -or $null -eq $health.health.duplicates) { throw "$shell/check invalid" }
        if ($schema.properties.schemaVersion.const -ne 1 -and $schema.properties.schemaVersion.minimum -ne 1) { throw "$shell/schema invalid" }
        if (!(Read-Result 'version').version -or !(Read-Result 'help').help.Contains('Exit codes') -or
            !(Read-Result 'command-help').help.Contains('Exit codes')) { throw "$shell/help or version invalid" }
        if ($shell -eq 'bash' -and (Read-Result 'slash').plan.entries[0].desired.arguments -cne '/flag "hello world" 中文') { throw 'Git Bash converted a raw slash argument' }
        Write-Output "PASS ${shell}: read-only examples, structured results, stdin, quoting, and Unicode"
    }
} finally {
    Set-Location -LiteralPath $originalDirectory
    $env:PATH = $originalPath
    $resolved = [IO.Path]::GetFullPath($root)
    if (!$resolved.StartsWith([IO.Path]::GetFullPath($temp).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Temp cleanup escaped its root.' }
    if ([IO.Directory]::Exists($resolved)) { [IO.Directory]::Delete($resolved, $true) }
}
