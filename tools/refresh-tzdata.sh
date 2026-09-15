#!/usr/bin/env bash
# Refresh standard-time/tzdata/tzdata.bin from the upstream IANA release.
#
# Usage:
#   tools/refresh-tzdata.sh            # query iana.org for latest version
#   tools/refresh-tzdata.sh 2026b      # use an explicit version tag
#
# Fetches the upstream tzdata source into a temp dir, runs the
# tzdata-compiler against it, writes the packed blob to
# standard-time/tzdata/tzdata.bin, and updates VERSION. The source
# files themselves are not committed — only the compiled blob ships.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TZDATA_DIR="$REPO_ROOT/standard-time/tzdata"
VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
    echo "Querying latest IANA version…"
    VERSION="$(curl -fsSL https://data.iana.org/time-zones/tzdb/version)"
fi
echo "IANA tzdata version: $VERSION"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading tzdata${VERSION}.tar.gz…"
curl -fsSL "https://data.iana.org/time-zones/releases/tzdata${VERSION}.tar.gz" \
    -o "$TMPDIR/tzdata.tar.gz"

echo "Extracting…"
mkdir -p "$TMPDIR/iana"
tar xzf "$TMPDIR/tzdata.tar.gz" -C "$TMPDIR/iana"

echo "Compiling packed blob…"
(cd "$REPO_ROOT" && \
    cargo run --quiet -p tzdata-compiler -- \
        "$TMPDIR/iana" \
        "$TZDATA_DIR/tzdata.bin")

echo "$VERSION" > "$TZDATA_DIR/VERSION"
echo
echo "Refreshed:"
echo "  $TZDATA_DIR/tzdata.bin"
echo "  $TZDATA_DIR/VERSION  (now $VERSION)"
