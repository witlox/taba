#!/usr/bin/env bash
set -euo pipefail

# Compute release version: YYYY.ADRcount.commitNr
#
# - YYYY:    current year (static, bumped manually for new series)
# - ADRcount: number of ADR files in docs/decisions/
# - commitNr: git commit count (from rev-list)
#
# Usage: ./scripts/set-version.sh [--dry-run]

DRY_RUN="${1:-}"

YEAR=$(date +%Y)
ADR_COUNT=$(find docs/decisions -name 'ADR-*.md' 2>/dev/null | wc -l | tr -d ' ')
COMMIT_COUNT=$(git rev-list --count HEAD)
FULL_VERSION="${YEAR}.${ADR_COUNT}.${COMMIT_COUNT}"

echo "Year:         ${YEAR}"
echo "ADR count:    ${ADR_COUNT}"
echo "Commit count: ${COMMIT_COUNT}"
echo "Full version: ${FULL_VERSION}"

if [ "$DRY_RUN" = "--dry-run" ]; then
    echo "(dry run — no files modified)"
    exit 0
fi

# Patch workspace Cargo.toml
sed -i.bak "s/^version = \".*\"/version = \"${FULL_VERSION}\"/" Cargo.toml
rm -f Cargo.toml.bak

# Patch workspace dependency versions for internal crates
sed -i.bak "s/version = \"[0-9]\{4\}\.[0-9]*\.[0-9]*\"/version = \"${FULL_VERSION}\"/g" Cargo.toml
rm -f Cargo.toml.bak

# Sync the lockfile's workspace-member versions ONLY.
# Never use `cargo generate-lockfile` — it re-resolves every
# third-party dependency to the newest compatible version,
# silently discarding committed pins.
cargo update --workspace --quiet

echo "Version set to ${FULL_VERSION}"
