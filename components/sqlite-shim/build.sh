#!/usr/bin/env bash
# Build sqlite-shim.wasm: the async-stackful SQLite component consumed by the
# standard-sqlite library.
#
# The shim is hand-written (no wit-bindgen): a wasm32-wasip1 core module whose
# exports are lifted `[async-lift-stackful]` during componentization, so
# SQLite's blocking p2 I/O runs on the export fiber and suspends there rather
# than stalling the caller's store.
#
# Requirements:
#   - wasi-sdk (https://github.com/WebAssembly/wasi-sdk), WASI_SDK_PATH set
#     or installed at ~/wasi-sdk-*
#   - Rust with the wasm32-wasip1 target (rustup target add wasm32-wasip1)
#   - wasm-tools on PATH
#   - wasi_snapshot_preview1.reactor.wasm (p1->p2 adapter) next to this script
#
# Output: standard-sqlite/artifacts/sqlite.wasm (checked in).
set -euo pipefail
cd "$(dirname "$0")"

if [[ -z "${WASI_SDK_PATH:-}" ]]; then
    WASI_SDK_PATH=$(ls -d "$HOME"/wasi-sdk-* 2>/dev/null | sort -V | tail -1)
fi
if [[ -z "$WASI_SDK_PATH" || ! -x "$WASI_SDK_PATH/bin/clang" ]]; then
    echo "error: wasi-sdk not found; set WASI_SDK_PATH" >&2
    exit 1
fi
echo "using wasi-sdk: $WASI_SDK_PATH"

ADAPTER="wasi_snapshot_preview1.reactor.wasm"
if [[ ! -f "$ADAPTER" ]]; then
    echo "error: $ADAPTER not found; download the wasi reactor adapter from a" >&2
    echo "       wasmtime release into $(pwd)" >&2
    exit 1
fi

export CC_wasm32_wasip1="$WASI_SDK_PATH/bin/clang"
export AR_wasm32_wasip1="$WASI_SDK_PATH/bin/llvm-ar"

cargo build --release --target wasm32-wasip1

CORE="target/wasm32-wasip1/release/sqlite_shim.wasm"
SQLITE_COMPONENT="../../standard-sqlite/artifacts/sqlite.wasm"
mkdir -p "$(dirname "$SQLITE_COMPONENT")"

# Componentize with a wit-component helper (the wasm-tools CLI only does
# async-stackful embed in --dummy mode, which discards the real module). The
# helper embeds the async world and lifts the `[async-lift-stackful]` exports,
# wiring p1 libc through the reactor adapter.
( cd componentize && cargo build --release )
componentize/target/release/componentize-sqlite \
    wit "$CORE" "$ADAPTER" "$SQLITE_COMPONENT"

ls -la "$SQLITE_COMPONENT"
echo "--- exported interface ---"
wasm-tools component wit "$SQLITE_COMPONENT" | head -40
