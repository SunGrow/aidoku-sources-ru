#!/usr/bin/env bash
#
# Package every source and assemble the source list into ./public.
#
# The output layout is what Aidoku expects from a source list host:
#   public/index.min.json   the list itself (this is the URL users add)
#   public/sources/*.aix    the packages, referenced relatively
#   public/icons/*.png
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LIST_NAME="${SOURCE_LIST_NAME:-Russian Manga & Comics}"

# `aidoku package` copies the first .wasm it finds in the build directory, so
# every source must keep its own target dir. A shared CARGO_TARGET_DIR would
# make sources pick up each other's binaries.
unset CARGO_TARGET_DIR

packages=()
for manifest in sources/*/Cargo.toml; do
	dir="$(dirname "$manifest")"
	echo "==> packaging ${dir##*/}"
	(cd "$dir" && aidoku package)
	packages+=("$dir/package.aix")
done

if [ ${#packages[@]} -eq 0 ]; then
	echo "no sources found under sources/" >&2
	exit 1
fi

echo "==> verifying ${#packages[@]} package(s)"
aidoku verify "${packages[@]}"

echo "==> building source list"
rm -rf public
aidoku build "${packages[@]}" -o public --name "$LIST_NAME"

echo
echo "source list written to public/index.min.json"
