#!/usr/bin/env bash
# Runs the Rust test suite on Linux inside a container, as a normal user.
#
# Usage (from the repository root, with Docker running):
#   bash scripts/linux-test.sh
#
# The source is copied into the container (tests write temporary files and the
# protocol tests regenerate TypeScript), the build folder lives in a named
# volume so repeated runs are incremental, and the desktop crate is skipped
# (it needs a display; CI covers its build).
set -euo pipefail

IMAGE="rust:1.90-bookworm"
VOLUME="wirecatch-linux-target"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

docker volume create "$VOLUME" >/dev/null

tar -C "$ROOT" \
  --exclude=./.git --exclude=./target --exclude=./node_modules --exclude='./apps/*/node_modules' \
  --exclude='./packages/*/node_modules' --exclude=./.claude --exclude=./.cache \
  --exclude='./apps/extension/.output' --exclude='./apps/desktop/build' \
  -cf - . |
docker run --rm -i \
  -v "$VOLUME:/target" \
  -e CARGO_TARGET_DIR=/target \
  -e CARGO_TERM_COLOR=never \
  "$IMAGE" bash -euo pipefail -c '
    mkdir -p /work && tar -C /work -xf -
    apt-get update -qq >/dev/null </dev/null
    apt-get install -y -qq --no-install-recommends ffmpeg >/dev/null </dev/null
    rustup component add clippy >/dev/null 2>&1
    useradd -m tester
    chown -R tester /work
    chown tester /target
    su tester -s /bin/bash -c "
      set -euo pipefail
      export PATH=/usr/local/cargo/bin:\$PATH CARGO_HOME=/target/cargo-home CARGO_TARGET_DIR=/target RUSTUP_HOME=/usr/local/rustup
      export WIRECATCH_TEST_FFMPEG=\$(command -v ffmpeg) WIRECATCH_REQUIRE_FFMPEG=1
      cd /work
      cargo clippy --workspace --exclude wirecatch-desktop --all-targets --features wirecatch-bridge/test-overrides -- -D warnings
      cargo test --no-fail-fast --workspace --exclude wirecatch-desktop --features wirecatch-bridge/test-overrides
    "
  '
