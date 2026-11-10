# OShot agent notes

- Build (release): `cargo build --release -p oshot-app`
- Bundle and sign: `./scripts/bundle.sh`
- Typecheck and lint: `cargo clippy --workspace --all-targets` (must be warning-free)
- Format: `cargo fmt --all`
- Tests: `cargo test --workspace`
- Visual snapshots and frame timing: `cargo test --release -p oshot-app --test visual -- --nocapture`

App flow tests drive real mouse and keyboard events through GPUI's test
platform with a fake macOS layer (`crates/oshot-app/tests/support`). Elements
tests click are tagged with `.debug_selector(...)`; add one to any new control.
