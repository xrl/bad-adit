# CLAUDE.md

## Build & Test

```sh
cd src-tauri
cargo fmt          # format before anything else
cargo clippy       # must pass with zero warnings
cargo test --lib   # run all unit tests
```

Always run all three before committing Rust changes.

## Releasing

Use `cargo-release` to bump versions and keep metadata in sync. From the repo root:

```sh
cd src-tauri
cargo release patch  # or minor/major — bumps Cargo.toml, tags, pushes
```

This keeps `Cargo.toml` version in sync. After `cargo release`, also update:
- `src-tauri/tauri.conf.json` — `"version"` field must match
- `xrl/homebrew-tap` Casks/bad-adit.rb — `version` field must match

The GitHub Actions release workflow triggers on `v*` tags and builds the DMG.

## Architecture

- Tauri v2 macOS menu bar app (no dock icon)
- Rust backend manages SSH tunnels with a TCP proxy layer for byte-counting stats
- Privileged ports (<1024) use a self-exec pattern: the binary re-runs itself as root via `osascript` with `--privileged-forwarder` flag
- The root forwarder has a parent-PID watchdog to prevent orphaned processes

## Key paths

- `src-tauri/src/` — all Rust source
- `src-tauri/tauri.conf.json` — Tauri config (version, bundle, window settings)
- `.github/workflows/release.yml` — CI release build (macOS ARM64 only)
- Homebrew tap: separate repo `xrl/homebrew-tap`, cask at `Casks/bad-adit.rb`
- Tunnel configs stored at `~/Library/Application Support/bad-adit/tunnels.json`
