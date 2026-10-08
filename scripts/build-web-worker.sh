#!/bin/sh
set -eu

cargo build --locked --release --target wasm32-unknown-unknown --package ocs_web_worker
stage="$TRUNK_STAGING_DIR"
# Trunk hands Windows hooks verbatim paths (\\?\C:\...) that POSIX sh cannot
# mkdir; strip the prefix and convert to a POSIX path first. No-op elsewhere.
case "$stage" in
  '\\?\'*) stage="${stage#"\\?\\"}" ;;
esac
command -v cygpath >/dev/null 2>&1 && stage=$(cygpath -u "$stage")
worker_out="$stage/worker_pkg"
mkdir -p "$worker_out"
wasm-bindgen \
  --target web \
  --out-dir "$worker_out" \
  --out-name ocs_web_worker \
  target/wasm32-unknown-unknown/release/ocs_web_worker.wasm
