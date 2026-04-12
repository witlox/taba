Pre-commit verification. Run this before every commit claim.

1. Format: `cargo fmt --all`
2. Clippy: `cargo clippy --workspace --all-targets -- -D warnings` — must be 0 errors
3. Deny: `cargo deny check` — must pass (if cargo-deny is installed)
4. Unit tests: `cargo test --workspace` — all must pass
5. Report: show pass/fail counts for each step

If ANY step fails, do NOT commit. Fix first, then re-run /project:verify.
