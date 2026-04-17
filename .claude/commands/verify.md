Pre-commit verification. Run this before every commit claim.

1. Format: `cargo fmt --all -- --check` — must pass
2. Clippy: `cargo clippy --workspace --all-targets --all-features -- -D warnings` — must be 0 warnings
3. Build: `cargo build --workspace` — must succeed
4. Deny: `cargo deny check` — must pass (if cargo-deny is installed)
5. Unit tests: `cargo test --workspace` — all must pass
6. Scenario coverage: check Gherkin scenarios against test implementations
7. Report: show pass/fail counts for each step

If ANY step fails, do NOT commit. Fix first, then re-run /project:verify.
