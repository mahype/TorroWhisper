# Continuous Integration

This document describes the automated checks that run against the repository. It is meant both as a reference for what to expect when opening a PR and as a starting point for maintaining or extending the pipeline.

## Overview

| Workflow | File | Trigger | Purpose |
| --- | --- | --- | --- |
| CI | [.github/workflows/ci.yml](../.github/workflows/ci.yml) | push to `main`, all PRs | Format, lint, test, build, dependency audit |
| CodeQL | [.github/workflows/codeql.yml](../.github/workflows/codeql.yml) | push to `main`, PRs to `main`, weekly cron | Static security analysis (SAST) for Swift |
| Release | [.github/workflows/release.yml](../.github/workflows/release.yml) | push of tag `v*` | Build universal `.app`, sign, notarize, smoke-test the DMG, publish release + appcast |
| Dependabot | [.github/dependabot.yml](../.github/dependabot.yml) | GitHub-side weekly scheduler | Open PRs for outdated Cargo and GitHub Actions dependencies |

Everything except Release and Dependabot is blocking for merges — a red CI or CodeQL run should be treated as a merge blocker.

## CI workflow (`ci.yml`)

Runs on `macos-15` with a 30-minute timeout. Concurrency is grouped by branch; pushing a new commit cancels an in-flight run on the same ref.

Steps, in order:

1. **Checkout** — `actions/checkout@v4`.
2. **Xcode 16** — `maxim-lobanov/setup-xcode@v1` pins Swift 6.
3. **Rust toolchain** — `dtolnay/rust-toolchain@stable` with `rustfmt` + `clippy`.
4. **Rust cache** — `Swatinem/rust-cache@v2`.
5. **`cargo fmt --all -- --check`** — fails on any unformatted Rust file.
6. **`cargo clippy --workspace --all-targets -- -D warnings`** — clippy with warnings as errors.
7. **`cargo test --workspace`** — runs Rust unit tests in both crates.
8. **`cargo audit`** — `rustsec/audit-check@v2`. Fails on any unpatched advisory in the dependency graph.
9. **`cargo deny`** — `cargo-deny` is installed via `taiki-e/install-action@v2` (the official `EmbarkStudios/cargo-deny-action` is a container action and therefore Linux-only, which does not fit our `macos-15` runner). Runs `check bans licenses sources` against [`deny.toml`](../deny.toml). Fails on license violations, banned crates, or unknown registries.
10. **SwiftLint** — installed via `brew install swiftlint`, runs against `apps/donny-macos/Sources`. Configured in [`.swiftlint.yml`](../.swiftlint.yml). Currently **non-strict** — warnings do not fail the build. See [Strictness roadmap](#strictness-roadmap) below.
11. **`swift format lint --recursive`** — Apple's swift-format (bundled with Xcode 16), configured in [`.swift-format`](../.swift-format). Also non-strict for now.
12. **`cargo build -p donny-bridge`** — produces the static lib that the Swift package links against.
13. **`swift build --package-path apps/donny-macos`** — compiles the Swift app against the Rust bridge.
14. **`swift test --package-path apps/donny-macos`** — runs the `DonnyTests` target. See [Swift tests](#swift-tests).

### Strictness roadmap

SwiftLint and swift-format are intentionally lenient on the first pass so the pipeline is not flooded with pre-existing violations. Once the Swift sources have been cleaned up, flip to strict:

- `swiftlint lint --strict ...` — treats warnings as errors.
- `swift format lint --strict ...` — same.

Until then, warnings show up as annotations on the PR (via the `github-actions-logging` reporter) without blocking merge.

### Swift tests

The package has one test target, `DonnyTests`, at [apps/donny-macos/Tests/DonnyTests/](../apps/donny-macos/Tests/DonnyTests/):

- **`SmokeTests.swift`** — minimum harness test; proves `swift test` works end-to-end.
- **`BridgeIntegrationTests.swift`** — calls `ow_validate_hotkey` through the FFI and checks the JSON envelope. Protects against accidental FFI symbol drift between Rust and Swift.

The test target links against the Rust static lib (`target/debug/libdonny_bridge.a`) and the same macOS frameworks as the main executable — see [Package.swift](../apps/donny-macos/Package.swift).

Locally, `swift test` requires **Xcode.app** (not just Command Line Tools) because `XCTest` ships with the Xcode developer toolchain:

```bash
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
    xcrun swift test --package-path apps/donny-macos
```

In CI, `maxim-lobanov/setup-xcode@v1` handles this automatically.

## CodeQL workflow (`codeql.yml`)

Static analysis for Swift, run against `macos-15`. Building the Swift target requires the Rust bridge, so the workflow builds `cargo build -p donny-bridge` before `swift build`.

Findings appear under **Security → Code scanning** in the GitHub UI. A scheduled run happens every Monday at 04:00 UTC so advisories that appear after the last commit still surface.

Rust language support in CodeQL is in public preview and is intentionally not enabled yet. Revisit once it reaches GA.

## Release workflow (`release.yml`)

Runs only on `v*` tags (e.g. `v0.2.2`, `v0.3.0-rc.1`). See [RELEASING.md](./RELEASING.md) for the maintainer-facing release procedure and the required secrets.

Relevant verification steps inside the release pipeline:

- **Tag / Cargo.toml version match** — fails early if the tag does not match the workspace version.
- **Codesign + notarization** — via [`scripts/codesign-macos.sh`](../scripts/codesign-macos.sh), including hardened runtime and stapling.
- **DMG smoke test** — [`scripts/smoke-test-dmg.sh`](../scripts/smoke-test-dmg.sh) mounts the finished DMG and runs:
  - `codesign --verify --deep --strict`
  - `spctl --assess --type execute` (Gatekeeper check)
  - `xcrun stapler validate` (notarization ticket intact)

If the smoke test fails, the release halts **before** the appcast update and the GitHub Release creation — broken DMGs never reach users.

## Dependabot

Configured in [.github/dependabot.yml](../.github/dependabot.yml):

- **`cargo`** ecosystem, weekly, up to 5 PRs. Grouped by patch/minor into a single "rust-deps" PR to reduce noise.
- **`github-actions`** ecosystem, weekly, up to 5 PRs. Grouped into one "actions" PR.

Every Dependabot PR runs the full CI workflow, so merging is straightforward once the pipeline is green.

Swift SPM updates (Sparkle) are **not** covered automatically — SPM dependabot requires a checked-in `Package.resolved`, which this repo does not use. Review Sparkle manually every few months.

## Local pre-commit hook (`lefthook.yml`)

Optional but recommended. Mirrors the CI checks locally, so a bad commit is caught before it reaches GitHub:

```bash
brew install lefthook
lefthook install
```

The hook runs on `pre-commit` for the staged files:

- `cargo fmt --check` on any staged `*.rs`.
- `swiftlint lint --quiet --strict` on staged Swift sources under `apps/donny-macos/Sources`.
- `swift format lint` on the same set.

Configuration: [`lefthook.yml`](../lefthook.yml). Uninstall: `lefthook uninstall`.

## Triggering checks manually

| Task | Command |
| --- | --- |
| Run the full CI suite on a branch | Open a PR — `ci.yml` fires automatically |
| Re-run a failed workflow | Actions tab → click the run → "Re-run all jobs" |
| Ad-hoc CodeQL run | Actions tab → CodeQL → "Run workflow" (main only) |
| Dry-run a release | Create an rc tag: `git tag v0.2.2-rc.1 && git push origin v0.2.2-rc.1` |

## When a check fails

- **`cargo fmt --check`** — run `cargo fmt --all` and commit.
- **`cargo clippy`** — fix the reported lints. Avoid blanket `#[allow(clippy::...)]` unless the lint is genuinely wrong; prefer a code change.
- **`cargo test`** — fix the test. Never merge a disabled test.
- **`cargo audit`** — bump the affected dependency. If no patched version exists, evaluate whether the advisory is exploitable for this app and document the decision in the PR.
- **`cargo deny` license failure** — either update [`deny.toml`](../deny.toml) to add the license to the `allow` list (if it is compatible with this project's license) or replace the offending crate.
- **`cargo deny` bans / sources** — investigate whether the new crate or git source is intentional.
- **SwiftLint / swift-format** — currently non-strict; warnings are informational. Once strict mode is on, run the tools locally and apply the auto-fix where possible (`swift format --in-place --recursive`).
- **`swift test`** — fix the test. The FFI integration test fails when a Rust symbol is renamed or a JSON envelope shape changes; update the matching C header at [apps/donny-macos/Bridge/DonnyBridgeFFI.h](../apps/donny-macos/Bridge/DonnyBridgeFFI.h) and the Swift call site together.
- **CodeQL** — treat findings as real. Resolve in code or, if a false positive, dismiss with justification in the Code scanning UI.
- **Release smoke test** — inspect `codesign`, `spctl`, or `stapler` output in the Actions log. Usually a signing identity problem (see [RELEASING.md](./RELEASING.md) §Troubleshooting) or a notarization delay.
