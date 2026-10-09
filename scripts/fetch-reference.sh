#!/bin/sh
# Fetches Tiled's sources, examples and test data into reference/tiled, at the commit the tests
# are pinned to. The folder is git-ignored: nothing of Tiled's is committed to this repository.
#
#   scripts/fetch-reference.sh                          from GitHub, or update reference/tiled
#   TILED_REPO=/path/to/tiled scripts/fetch-reference.sh  from another clone of Tiled
#
# Updating Tiled's version is changing PINNED.
set -eu

PINNED=221be2066c4b0ed4bbefbcdfe25d0ad952fc51bd
REPO=${TILED_REPO:-https://github.com/mapeditor/tiled.git}

root=$(cd "$(dirname "$0")/.." && pwd)
dest="$root/reference/tiled"

if [ -d "$dest/.git" ]; then
  if [ "$(git -C "$dest" rev-parse HEAD 2>/dev/null)" = "$PINNED" ]; then
    echo "reference/tiled is at $PINNED"
    exit 0
  fi
  git -C "$dest" cat-file -e "$PINNED^{commit}" 2>/dev/null || git -C "$dest" fetch --quiet origin
else
  mkdir -p "$root/reference"
  # Blobs come on checkout, for the folders below only.
  git clone --quiet --filter=blob:none --no-checkout "$REPO" "$dest"
fi

git -C "$dest" sparse-checkout set --cone docs examples src tests
git -C "$dest" -c advice.detachedHead=false checkout --quiet "$PINNED"
printf '%s\n' "$PINNED" > "$root/reference/PINNED"
echo "reference/tiled is at $PINNED"
