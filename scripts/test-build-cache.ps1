#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$temp = if ($IsWindows) { [Environment]::GetEnvironmentVariable('TEMP', 'Machine') } else { $env:TMPDIR }
if (!$temp) { $temp = if ($IsWindows) { [Environment]::GetEnvironmentVariable('TEMP', 'User') } else { '/tmp' } }
$fixture = Join-Path $temp "build-cache-test-$([Guid]::NewGuid().ToString('N'))"
$repo = Join-Path $fixture 'project'
$hostPath = Join-Path $PSHOME $(if ($IsWindows) { 'pwsh.exe' } else { 'pwsh' })
$cache = $null
try {
    [IO.Directory]::CreateDirectory("$repo/scripts") | Out-Null
    $wrapper = "$repo/scripts/with-build-cache.ps1"
    Copy-Item -LiteralPath "$PSScriptRoot/with-build-cache.ps1" -Destination $wrapper
    if ($IsWindows) {
        $source = [IO.File]::ReadAllText($wrapper)
        $source = $source.Replace("[Environment]::GetEnvironmentVariable('TEMP', 'Machine')", "'$fixture/missing'")
        $source = $source.Replace("[Environment]::GetEnvironmentVariable('TEMP', 'User')", "'$($temp.Replace("'", "''"))'")
        [IO.File]::WriteAllText($wrapper, $source, [Text.UTF8Encoding]::new($false))
    }
    $output = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command '[IO.File]::WriteAllText((Join-Path $env:CARGO_BUILD_BUILD_DIR "probe"), "ok"); Write-Output $env:CARGO_BUILD_BUILD_DIR'
    if ($LASTEXITCODE) { throw "First command failed: $output" }
    $cache = $output[-1].Trim()
    if ([IO.Path]::GetDirectoryName($cache) -ne [IO.Path]::GetFullPath($temp).TrimEnd([IO.Path]::DirectorySeparatorChar) -or !(Test-Path -LiteralPath "$cache/probe")) { throw 'Output did not use system temp.' }
    [IO.Directory]::SetLastWriteTimeUtc($cache, [DateTime]::UtcNow.AddDays(-8))
    $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 0'
    if ($LASTEXITCODE -or (Test-Path -LiteralPath "$cache/probe")) { throw 'Expired cache was not pruned.' }
    # Use a tiny threshold in this disposable copy to exercise the same size branch.
    [IO.File]::WriteAllText($wrapper, [IO.File]::ReadAllText($wrapper).Replace('8GB', '128'), [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllBytes("$cache/large", [byte[]]::new(256))
    $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 0'
    if ($LASTEXITCODE -or (Test-Path -LiteralPath "$cache/large")) { throw 'Oversized cache was not pruned.' }
    [IO.File]::WriteAllText("$cache/held", 'keep')
    [IO.Directory]::SetLastWriteTimeUtc($cache, [DateTime]::UtcNow.AddDays(-8))
    $held = [IO.File]::Open("$cache/held", 'Open', 'Read', 'None')
    try {
        $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 0' 2>&1
        if (!$LASTEXITCODE -or !(Test-Path -LiteralPath "$cache/held")) { throw 'Held cache was not preserved.' }
    } finally { $held.Dispose() }
    [IO.Directory]::SetLastWriteTimeUtc($cache, [DateTime]::UtcNow)
    $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 23'
    if ($LASTEXITCODE -ne 23) { throw 'Tool exit status was lost.' }
    [IO.File]::WriteAllText("$fixture/arguments.ps1", 'if (($args -join "|") -ne "--|--locked") { exit 17 }', [Text.UTF8Encoding]::new($false))
    $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -File "$fixture/arguments.ps1" -- --locked
    if ($LASTEXITCODE) { throw 'Tool argument separator was not preserved.' }
    $busy = [IO.File]::Open("$cache.lock", 'Open', 'ReadWrite', 'None')
    try {
        $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 0' 2>&1
        if (!$LASTEXITCODE) { throw 'A busy project cache was not rejected.' }
    } finally { $busy.Dispose() }
    $owner = [IO.File]::ReadAllText("$cache/.project-root")
    [IO.File]::WriteAllText("$cache/.project-root", 'another checkout')
    $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 0' 2>&1
    if (!$LASTEXITCODE -or !(Test-Path -LiteralPath "$cache/held")) { throw 'Foreign cache was not preserved.' }
    [IO.File]::WriteAllText("$cache/.project-root", $owner)
    if ($IsWindows) {
        [IO.Directory]::CreateDirectory("$fixture/linked") | Out-Null
        $null = New-Item -ItemType Junction -Path "$cache/link" -Target "$fixture/linked"
        try {
            $null = & $hostPath -NoProfile -File $wrapper $hostPath -NoProfile -Command 'exit 0' 2>&1
            if (!$LASTEXITCODE -or !(Test-Path -LiteralPath "$fixture/linked")) { throw 'Linked cache was not rejected.' }
        } finally { Remove-Item -LiteralPath "$cache/link" -Force }
    }
    [IO.File]::WriteAllText("$repo/Cargo.toml", "[package]`nname = `"cache-probe`"`nversion = `"0.1.0`"`nedition = `"2021`"`n", [Text.UTF8Encoding]::new($false))
    [IO.Directory]::CreateDirectory("$repo/src") | Out-Null
    [IO.File]::WriteAllText("$repo/src/main.rs", 'fn main() {}', [Text.UTF8Encoding]::new($false))
    $null = & $hostPath -NoProfile -File $wrapper cargo build --offline 2>&1
    if ($LASTEXITCODE) { throw 'Cargo build failed.' }
    $binary = "$repo/target/debug/cache-probe" + $(if ($IsWindows) { '.exe' } else { '' })
    if (!(Test-Path -LiteralPath $binary) -or !(Test-Path -LiteralPath "$cache/debug/.fingerprint")) { throw 'Cargo did not separate final and intermediate artifacts.' }
    Write-Output 'Build cache checks passed: temp fallback, age, size, held files, busy lock, ownership, reparse, exit status, Cargo output separation.'
} finally {
    if ($cache -and [IO.Path]::GetDirectoryName($cache) -eq [IO.Path]::GetFullPath($temp).TrimEnd([IO.Path]::DirectorySeparatorChar) -and (Split-Path $cache -Leaf) -match '^project-cargo-[a-f0-9]{16}$') {
        if (Test-Path -LiteralPath $cache) { Remove-Item -LiteralPath $cache -Recurse -Force }
        if (Test-Path -LiteralPath "$cache.lock") { Remove-Item -LiteralPath "$cache.lock" -Force }
    }
    if ([IO.Path]::GetDirectoryName($fixture) -eq [IO.Path]::GetFullPath($temp).TrimEnd([IO.Path]::DirectorySeparatorChar)) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
