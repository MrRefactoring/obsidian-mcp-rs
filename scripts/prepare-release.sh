#!/usr/bin/env bash
# prepare-release.sh — bump all version strings and create a release commit + tag
#
# Usage:
#   ./scripts/prepare-release.sh <new-version>
#
# Example:
#   ./scripts/prepare-release.sh 0.2.0

set -euo pipefail

NEW_VERSION="${1:-}"

if [[ -z "$NEW_VERSION" ]]; then
  echo "Usage: $0 <new-version>"
  echo "Example: $0 0.2.0"
  exit 1
fi

# Validate semver format (major.minor.patch)
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: version must be in semver format (e.g. 1.2.3), got: $NEW_VERSION"
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Detect current version from Cargo.toml (line 3: version = "x.y.z")
CURRENT_VERSION=$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $CURRENT_VERSION"
echo "New version:     $NEW_VERSION"
echo ""

if [[ "$CURRENT_VERSION" == "$NEW_VERSION" ]]; then
  echo "Error: new version ($NEW_VERSION) is the same as current ($CURRENT_VERSION)"
  exit 1
fi

# ── 1. Cargo.toml ──────────────────────────────────────────────────────────────
echo "Updating Cargo.toml..."
sed -i '' "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml

# ── 2. Cargo.lock ──────────────────────────────────────────────────────────────
echo "Updating Cargo.lock..."
cargo update --precise "$NEW_VERSION" obsidian-mcp-rs 2>/dev/null || cargo generate-lockfile

# ── 3. npm packages ────────────────────────────────────────────────────────────
echo "Updating npm package versions..."

PLATFORM_DIRS=(
  npm/obsidian-mcp-rs
  npm/darwin-arm64
  npm/darwin-x64
  npm/linux-arm64
  npm/linux-x64
  npm/linux-x64-musl
  npm/win32-arm64
  npm/win32-x64
)

for dir in "${PLATFORM_DIRS[@]}"; do
  pkg="$dir/package.json"
  if [[ -f "$pkg" ]]; then
    sed -i '' "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" "$pkg"
    # Update optionalDependencies versions in the main wrapper
    sed -i '' "s/\"@obsidian-mcp-rs\\/[^\"]*\": \"$CURRENT_VERSION\"/$(
      grep -o '"@obsidian-mcp-rs/[^"]*": "'"$CURRENT_VERSION"'"' "$pkg" | \
      sed "s/$CURRENT_VERSION/$NEW_VERSION/g"
    )/g" "$pkg" 2>/dev/null || true
    echo "  Updated $pkg"
  fi
done

# Re-run npm install in wrapper to update package-lock.json
echo "Updating package-lock.json..."
(cd npm/obsidian-mcp-rs && npm install --package-lock-only --silent)

# ── 4. CHANGELOG.md — ensure entry exists ──────────────────────────────────────
TODAY=$(date +%Y-%m-%d)
if grep -q "\[${NEW_VERSION}\]" CHANGELOG.md; then
  echo "CHANGELOG.md already has an entry for $NEW_VERSION — skipping."
else
  echo "Adding CHANGELOG.md placeholder for $NEW_VERSION..."
  # Insert new section after the first line (# Changelog)
  PLACEHOLDER="## [$NEW_VERSION] - $TODAY

### Added

-

### Changed

-

### Fixed

-

"
  # Use Python for reliable multi-line insertion
  python3 - <<PYEOF
import re, pathlib
content = pathlib.Path("CHANGELOG.md").read_text()
marker = "# Changelog\n"
idx = content.index(marker) + len(marker)
new_content = content[:idx] + "\n" + """$PLACEHOLDER""" + content[idx:]
pathlib.Path("CHANGELOG.md").write_text(new_content)
PYEOF
  echo "  Placeholder added — fill in the details before committing."
fi

# ── 5. Verify ──────────────────────────────────────────────────────────────────
echo ""
echo "Version strings after update:"
grep -m1 '^version = ' Cargo.toml
grep '"version"' npm/obsidian-mcp-rs/package.json | head -1
grep '"version"' npm/darwin-arm64/package.json | head -1

echo ""
echo "Done. Next steps:"
echo "  1. Fill in CHANGELOG.md entry for v$NEW_VERSION"
echo "  2. Review: git diff"
echo "  3. Commit: git add -A && git commit -m 'chore: release v$NEW_VERSION'"
echo "  4. Tag:    git tag v$NEW_VERSION && git push origin master --tags"
