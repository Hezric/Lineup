# Contributing to Lineup

Use GitHub Issues in `Hezric/Lineup`: specifications are parent issues and tickets are native sub-issues. Ticket labels are `ready-for-agent`, `in-progress`, `blocked`, and `done`; accepted tickets close with a PR/commit and verification. Current behavior lives in `docs/features/` when needed. See [AGENTS.md](AGENTS.md) for the complete agent workflow.

Write public contributions in English. Keep `README_zh-Hans.md` synchronized when changing `README.md`.

1. Create a branch named `<type>/<short-desc>`, such as `fix/handle-empty-input`.
2. Sign every commit with a GitHub-registered signing key. Use scoped Conventional Commits: `fix(parser): handle empty input`. Lowercase where meaning permits; use backticks for technical tokens. PR titles follow the same format. Supported types are feat, fix, perf, refactor, docs, style, test, build, ci, chore, and revert.
3. Run `cargo test -j 2 --locked` and `cargo build --release --locked`. Open one PR per coherent stage and explain its behavior and verification.
4. Rebase stale branches locally onto main, re-sign rewritten commits, and use `--force-with-lease` only on the development branch. Wait for `test` and `policy` to pass again. Maintainers squash-merge through GitHub using the PR title and body; GitHub signs the resulting commit and deletes the merged branch.

Main requires tested signed commits, strict up-to-date checks, linear history, and forbids force pushes or deletion. Direct pushes require the same signed SHA to have passed CI on another branch against current main. Bots use that flow or open PRs. Dependabot's native branch names are exempt from the branch check; its scoped messages, signatures, and tests remain required. Weekly grouped updates auto squash-merge after green CI.

Maintainers release only on explicit user authorization: git-cliff prepares the next SemVer and `CHANGELOG.md`, a `chore(release): vX.Y.Z` PR updates the version, and its merged commit receives a signed annotated tag. The tag workflow builds artifacts plus SHA256 and publishes that changelog section. `v*` tags cannot be updated or deleted. The single `Release due` issue reminds immediately for security or breaking changes, at five unreleased user-visible changes, or at one after 30 days; it does not trigger a release.

Report vulnerabilities privately using the repository Security tab. Contributions use the repository's MIT license.
