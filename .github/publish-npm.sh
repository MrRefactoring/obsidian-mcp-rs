#!/usr/bin/env bash
set -euo pipefail

name="$(node -p "require('./package.json').name")"
version="$(node -p "require('./package.json').version")"

if npm view "$name@$version" version >/dev/null 2>&1; then
  echo "$name@$version is already published; skipping"
  exit 0
fi

npm publish --access public --provenance
