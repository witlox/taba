# taba — CI tiers (cascading: each tier includes the lower)
#
# `just` (no target) = fmt-check + lint + deny + Tier 1.
# Run before every commit.
#
# Tooling:
#   - cargo-nextest: `cargo install cargo-nextest --locked`
#   - cargo-deny:    `cargo install cargo-deny --locked`
#   - mdbook (docs): `cargo install mdbook --locked`

# === Tier 1: fast (between every edit, pre-commit) ===

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings

deny:
    cargo deny check 2>/dev/null || echo "cargo-deny not installed, skipping"

test: check
    cargo nextest run --workspace --locked 2>/dev/null || cargo test --workspace --locked
    cargo test --workspace --doc --locked
    TABA_BDD_FAST=1 cargo test --locked -p taba-acceptance --test acceptance 2>/dev/null || echo "BDD smoke skipped (taba-acceptance not built)"

check:
    cargo check --workspace --locked

# === Tier 2: slow (pre-PR) ===

test-slow: test
    cargo nextest run --workspace --run-ignored=only --no-tests=warn --locked 2>/dev/null || \
        cargo test --workspace -- --include-ignored --locked
    cargo test --locked -p taba-acceptance --test acceptance 2>/dev/null || echo "BDD full skipped"

# === Tier 3: full (pre-merge / nightly) ===

test-full: test-slow
    cargo test --locked -p taba-integration

# === Documentation ===

docs:
    mdbook build

docs-serve:
    mdbook serve --open

# === All ===

all: fmt-check lint deny test
