# taba — CI tiers (cascading: each tier includes the lower)
# `just` (no target) = fmt-check + lint + Tier 1

# === Tier 1: fast (between every edit, pre-commit) ===

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

check:
    cargo check --workspace

test: check
    cargo test --workspace

# === Tier 2: slow (pre-PR) ===

test-slow: test
    cargo test --workspace -- --include-ignored

# === Tier 3: full (pre-merge / nightly) ===

test-full: test-slow
    @echo "e2e tests: no e2e harness yet (M4+)"

# === Security ===

deny:
    cargo deny check 2>/dev/null || echo "cargo-deny not installed, skipping"

# === All ===

all: fmt-check lint test deny
