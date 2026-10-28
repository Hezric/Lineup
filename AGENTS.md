# Lineup

Public source, issues, PRs, commits, and documentation are English; `README_zh-Hans.md` is the synchronized Chinese version of authoritative `README.md`, phrased natively rather than translated literally. The README and the GitHub About description keep a literary, humane voice; commands, options, and facts stay exact.

## Delivery and tracker

GitHub Issues in `Hezric/Lineup` is the formal tracker. A specification is a parent issue; implementation tickets are native sub-issues. Use `gh issue create` and `gh api repos/Hezric/Lineup/issues/<parent>/sub_issues -F sub_issue_id=<child-database-id>` to attach them. Dependencies are explicit issue links in the body. This declaration is complete: no separate tracker setup document is required, and formal delivery stays in Issues rather than `.scratch/`.

Labels map ticket state: `ready-for-agent` -> `in-progress` -> `done`; a blocked ticket uses `blocked` with the exact prerequisite. Accepting completion closes the ticket and records its commit/PR plus verification; close the parent when every sub-issue is accepted. Use Git and Issues for progress. Canonical domain language belongs in root `GLOSSARY.md` when needed, architectural rationale in `docs/adr/`, and delivered behavior in `docs/features/`. Create these lazily for real consumers.

## Git and checks

- Work on `<type>/<short-desc>` branches, for example `feat/add-parser`; the type matches the change. One PR per coherent stage.
- Every commit and PR title uses lowercase `type(scope): subject` (or `type(scope)!:` for breaking changes). Preserve meaningful technical casing and put technical tokens in backticks where useful. Allowed types and exact validation live in `cchk.toml`. First commit: `chore(repo): initial commit`.
- Sign every commit and release tag with a GitHub-registered signing key. Configure an allowed-signers file locally to verify SSH signatures. Never turn off signing to work around an unavailable key.
- Run `cargo test -j 2 --locked` and `cargo build --release --locked` before proposing completion. CI requires `test` and `policy`, bound to GitHub Actions. Preserve both names when adding platform jobs.
- Main is signed and linear, forbids force pushes and deletion, and requires strict up-to-date checks. Agent work defaults to a PR, squash-merged through GitHub after green checks. Squash title is the PR title; body is the PR body. GitHub signs the resulting commit; read back verification. Merged branches are automatically deleted.
- Update a stale PR by rebasing locally onto `origin/main`, re-signing rewritten commits, and pushing with `--force-with-lease` to its development branch. Re-run checks. Use GitHub squash merging; update branches locally.
- A direct push must fast-forward main to the exact signed SHA already green on another branch and based on current main. Bot automation follows the same requirement or opens a PR. Keep the main ruleset free of bypass actors.
- Dependabot branches keep their native `dependabot/` names; their branch check alone is exempt. Scoped messages, tests, signatures, and squash merge still apply. Group updates weekly by ecosystem and allow automatic squash merge after CI.

## Release

Release only when the user explicitly asks. Mention the `Release due` issue when present. The reminder opens or updates one issue for a security fix (`fix(security):`), breaking change, five unreleased feat/fix/perf changes, or at least one such change after 30 days. It never releases automatically.

Use git-cliff with `cliff.toml` to determine the next SemVer and update `CHANGELOG.md`; bump `Cargo.toml and Cargo.lock` consistently in a `chore(release): vX.Y.Z` PR. After its tested squash merge, verify the commit, create and verify a signed annotated `vX.Y.Z` tag on that commit, then push the tag. The release workflow tests, builds artifacts and SHA256, and publishes notes from that version's changelog section. Verify the Release and downloaded checksums. Protected `v*` tags are immutable; fix a bad release with a new version.
