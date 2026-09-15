#!/usr/bin/env bash
# Refresh standard-crypto-roots/roots/cacert.pem from the curl/Mozilla bundle.
#
# Usage:
#   tools/refresh-ca-roots.sh
#
# Downloads the latest Mozilla-derived CA root bundle published by the curl
# project (extracted from NSS certdata.txt) and writes it to
# standard-crypto-roots/roots/cacert.pem. The PEM is embedded verbatim into
# the standard.crypto.roots package as a passive WASM data segment and parsed
# on demand by `Roots.mozillaRoots()`.
#
# Provenance: https://curl.se/docs/caextract.html
# License:    Mozilla Public License 2.0 (MPL-2.0) — the certificate data
#             originates from Mozilla's NSS root store.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOTS_DIR="$REPO_ROOT/standard-crypto-roots/roots"
DEST="$ROOTS_DIR/cacert.pem"

mkdir -p "$ROOTS_DIR"

echo "Downloading curl/Mozilla CA bundle…"
curl -fsSL https://curl.se/ca/cacert.pem -o "$DEST"

COUNT="$(grep -c 'BEGIN CERTIFICATE' "$DEST")"
echo
echo "Refreshed:"
echo "  $DEST  ($COUNT certificates)"
