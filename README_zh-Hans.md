<div align="center">
  <h1>🎼 Lineup</h1>
  <p><a href="https://github.com/Hezric/Lineup/actions/workflows/ci.yml"><img src="https://github.com/Hezric/Lineup/actions/workflows/ci.yml/badge.svg" alt="CI"></a> <a href="https://github.com/Hezric/Lineup/releases"><img src="https://img.shields.io/github/v/release/Hezric/Lineup" alt="发布版本"></a> <a href="LICENSE"><img src="https://img.shields.io/github/license/Hezric/Lineup" alt="许可证"></a></p>
  <p><a href="README.md">English</a> | 简体中文</p>
  <p>让 Windows 的开机时光井然有序，也让一天从容开始。</p>
</div>

Lineup 为脚本和编程代理打理 Windows 启动程序。用 JSON 写下期望的状态，先查看计划，再从容应用。每次调用都会返回 JSON。

## 功能

- 查看启动项，检查失效目标与重复程序。
- 预览并应用清单，也可单独移除或运行启动项。
- 管理当前用户和全局启动项，包括延迟与提权任务。

## 安装

在 Windows 11 x64 上，通过 [ScoopZephyr bucket](https://github.com/Hezric/ScoopZephyr) 安装：

```powershell
scoop bucket add zephyr https://github.com/Hezric/ScoopZephyr
scoop install zephyr/lineup
```

也可从 [GitHub Releases](https://github.com/Hezric/Lineup/releases) 下载 Windows x64 压缩包，解压后将 `lineup.exe` 放入 PATH。若从源码构建，请安装 rustup 管理的 Rust 和 MSVC 构建工具，运行 `cargo build --release --locked`；可执行文件位于 `target/release/`。Lineup 不会将自身设为启动项，也不会常驻运行。

## 快速开始

将清单保存为 `startup.json`：

```json
{
  "schemaVersion": 1,
  "entries": [
    { "name": "example", "path": "%SystemRoot%\\System32\\notepad.exe" }
  ]
}
```

```powershell
lineup list
lineup check
lineup plan --manifest startup.json
lineup apply --manifest startup.json
lineup remove example
lineup run example
lineup schema
lineup --help
lineup list --help
lineup --version
```

应用前请先检查计划。修改全局启动项或提权任务需要管理员令牌；Lineup 不会自行请求提权。应用失败时，启动项可能已部分改变，请检查 JSON 报告后再重试。

## 文档

[使用指南](docs/usage.md)介绍命令、清单字段、提权、退出码和开发检查；[清单 schema](manifest.schema.json)可供编辑器使用，[当前行为](docs/features/startup-management.md)说明底层约定。

## 参与贡献

参阅 [CONTRIBUTING.md](CONTRIBUTING.md)。安全问题请通过 Security 页面私密报告。

## 许可证

[MIT](LICENSE) © Hezric
