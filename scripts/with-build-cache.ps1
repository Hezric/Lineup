#requires -Version 7.0
$Tool = $args[0]
$ToolArguments = @($args | Select-Object -Skip 1)
if (!$Tool) { throw 'Specify the build tool and its arguments.' }
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$identity = if ($IsWindows) { $repo.ToLowerInvariant() } else { $repo }
$hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity))).Substring(0, 16).ToLowerInvariant()
$name = (Split-Path $repo -Leaf).ToLowerInvariant() -replace '[^a-z0-9-]', '-'
$toolPath = (Get-Command $Tool -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$candidates = if ($IsWindows) {
    [Environment]::GetEnvironmentVariable('TEMP', 'Machine')
    [Environment]::GetEnvironmentVariable('TEMP', 'User')
} else { $(if ($env:TMPDIR) { $env:TMPDIR } else { '/tmp' }) }
$lock = $null
foreach ($candidate in $candidates) {
    if (!$candidate) { continue }
    try {
        $temp = [IO.Path]::GetFullPath($candidate)
        if (![IO.Directory]::Exists($temp)) { throw 'Directory does not exist.' }
        $cache = Join-Path $temp "$name-cargo-$hash"
        if ([IO.Path]::GetDirectoryName($cache) -ne $temp.TrimEnd([IO.Path]::DirectorySeparatorChar)) { throw 'Cache escaped system temp.' }
        $lock = [IO.File]::Open("$cache.lock", 'OpenOrCreate', 'ReadWrite', 'None')
        break
    } catch {
        $reason = $_.Exception
        while ($reason.InnerException) { $reason = $reason.InnerException }
        if (($reason.HResult -band 0xffff) -in 32, 33) { throw 'The project build cache is busy.' }
        Write-Warning "Cannot use temp candidate $candidate : $($reason.Message)"
    }
}
if (!$lock) { throw 'No writable system temp is available, or the project cache is busy.' }
$previous = $env:CARGO_BUILD_BUILD_DIR
$location = Get-Location
try {
    $files = [Collections.Generic.List[IO.FileInfo]]::new()
    if (Test-Path -LiteralPath $cache) {
        if ((Get-Item -LiteralPath $cache -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Cache is a reparse point.' }
        $pending = [Collections.Generic.Stack[string]]::new()
        $pending.Push($cache)
        while ($pending.Count) {
            foreach ($entry in [IO.DirectoryInfo]::new($pending.Pop()).EnumerateFileSystemInfos()) {
                if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Cache contains a reparse point: $($entry.FullName)" }
                if ($entry -is [IO.DirectoryInfo]) { $pending.Push($entry.FullName) } else { $files.Add($entry) }
            }
        }
        if (!(Test-Path -LiteralPath "$cache/.project-root") -or [IO.File]::ReadAllText("$cache/.project-root") -ne $identity) { throw 'Cache ownership does not match this checkout.' }
        $bytes = ($files | Measure-Object Length -Sum).Sum
        if ($bytes -gt 8GB -or [IO.Directory]::GetLastWriteTimeUtc($cache) -lt [DateTime]::UtcNow.AddDays(-7)) {
            foreach ($file in $files) {
                $held = [IO.File]::Open($file.FullName, 'Open', 'Read', 'None')
                $held.Dispose()
            }
            Remove-Item -LiteralPath $cache -Recurse -Force
            Write-Host "Pruned project build cache ($bytes bytes)."
        }
    }
    [IO.Directory]::CreateDirectory($cache) | Out-Null
    [IO.File]::WriteAllText("$cache/.project-root", $identity, [Text.UTF8Encoding]::new($false))
    $env:CARGO_BUILD_BUILD_DIR = $cache
    Write-Host "Project build cache: $cache (prune before use: 8 GiB / 7 days)."
    Set-Location -LiteralPath $repo
    & $toolPath @ToolArguments
    $result = $LASTEXITCODE
    [IO.Directory]::SetLastWriteTimeUtc($cache, [DateTime]::UtcNow)
} finally {
    Set-Location -LiteralPath $location
    $env:CARGO_BUILD_BUILD_DIR = $previous
    $lock.Dispose()
}
exit $result
