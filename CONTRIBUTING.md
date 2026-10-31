# Contributing to Polyglass

Thanks for helping! A few ground rules keep the project private-by-default and easy to review.

## Before you start

- Read [SPEC.md](./SPEC.md) for requirements (each has an ID like `CAP-03`) and
  [CHECKLIST.md](./CHECKLIST.md) for what's in progress.
- Pick an unclaimed task, mark it `[~]` in CHECKLIST.md in your PR, and reference requirement IDs
  in commit messages and tests (e.g. a test named `cap03_stale_results_are_never_drawn`).

## Development

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass; CI runs them on Windows, macOS and Linux.

## Rules that are easy to miss

- **No text in logs.** Never log recognized or translated text at the default level (PRIV-07).
  Types that carry text implement `Debug` without it.
- **No images on disk.** Frames and snapshots stay in memory (PRIV-02).
- **No network in on-device mode.** Anything that touches the network goes through an online
  backend the user enabled, or a user-started model download (PRIV-01).
- **No telemetry**, analytics or automatic crash upload (PRIV-06).
- **Headless core.** `crates/pg-*` must not depend on Tauri (PLAN principle 4).
- **Models** are only added via `models/manifest.toml` with URL, SHA-256 and license.
- **Fixtures** must be created by you and released as CC0-1.0.

## Sign-off

We use the [Developer Certificate of Origin](https://developercertificate.org/). Add
`Signed-off-by: Your Name <you@example.com>` to each commit (`git commit -s`). No CLA.

## Architecture decisions

Significant choices get an ADR in `docs/adr/` (copy `0000-template.md`).
