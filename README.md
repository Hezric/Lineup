<div align="center">
  <h1>🎼 Lineup</h1>
  <p><a href="https://github.com/Hezric/Lineup/actions/workflows/ci.yml"><img src="https://github.com/Hezric/Lineup/actions/workflows/ci.yml/badge.svg" alt="CI"></a> <a href="https://github.com/Hezric/Lineup/releases"><img src="https://img.shields.io/github/v/release/Hezric/Lineup" alt="Release"></a> <a href="LICENSE"><img src="https://img.shields.io/github/license/Hezric/Lineup" alt="License"></a></p>
  <p>English | <a href="README_zh-Hans.md">简体中文</a></p>
  <p>Give Windows startup a little order, and your day a quieter beginning.</p>
</div>

Lineup helps scripts and coding agents tend the programs Windows starts. Describe the entries you want in TOML or JSON, see the plan, and apply it when you are ready. Every invocation returns JSON.

## Features

- Inspect startup entries and check for missing targets and duplicates.
- Plan and apply a manifest, or remove and run individual entries.
- Manage current-user and machine entries, including delayed and elevated tasks.

## Install

On Windows 11 x64, install from the [ScoopZephyr bucket](https://github.com/Hezric/ScoopZephyr):

```powershell
scoop bucket add zephyr https://github.com/Hezric/ScoopZephyr
scoop install zephyr/lineup
```

Alternatively, download the Windows x64 zip from [GitHub Releases](https://github.com/Hezric/Lineup/releases), extract it, and put `lineup.exe` on your PATH. To build from source, install Rust with rustup and the MSVC build tools, then run `cargo build --release --locked`; the executable is in `target/release/`. Lineup does not register itself for startup or stay resident.

## Use with an agent

Give your coding agent a quiet place to start:

```text
Install Lineup from https://github.com/Hezric/Lineup on this Windows machine:
scoop bucket add zephyr https://github.com/Hezric/ScoopZephyr
scoop install zephyr/lineup
Verify with lineup --version.

Install its companion skill through my existing skill management. If I have
none, ask me first and recommend npx skills add Hezric/Lineup -g (Vercel's
skills CLI, https://github.com/vercel-labs/skills; Node.js 22.20+). Verify the
installed SKILL.md, then use the skill to inspect startup without changing it.
```

## Quick start

Save a manifest as `startup.toml`:

```toml
schemaVersion = 1

[[entries]]
name = 'example'
path = '%SystemRoot%\System32\notepad.exe' # Windows path
```

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

Review the plan before applying. Machine changes and elevated entries require an administrator token; Lineup does not prompt for elevation. A failed apply can leave an entry partially changed, so inspect its JSON report before retrying.

## Documentation

The [usage guide](docs/usage.md) covers commands, manifest fields, elevation, exit codes, and development checks. The [manifest schema](manifest.schema.json) is available for editors, and [current behavior](docs/features/startup-management.md) describes the underlying contracts.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Report security issues privately through the Security tab.

## License

[MIT](LICENSE) © Hezric
