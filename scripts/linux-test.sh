#!/usr/bin/env bash
# Runs the Rust gate on Linux inside a container, as a normal user, with a real ffmpeg.
#
# Usage (from the repository root, with Docker running):
#   bash scripts/linux-test.sh
#
# It runs the Rust group of the gate (scripts/gate.mjs) without the desktop crate, with the Rust release rust-toolchain.toml names (the latest stable) as CI does. Debian's Node is older than the repository's minimum; the gate script is written to run on it. The source is copied into the container (tests write temporary files, and the protocol tests export TypeScript into target/ts-rs), and the build folder lives in a named volume so repeated runs are incremental. The desktop crate needs a display and system WebKit packages, so it is left to CI and the Windows run.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="rust:bookworm"
VOLUME="wirecatch-linux-target"

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
    apt-get install -y -qq --no-install-recommends ffmpeg nodejs >/dev/null </dev/null
    (cd /work && rustup toolchain install) >/dev/null 2>&1
    useradd -m tester
    chown -R tester /work
    chown tester /target
    su tester -s /bin/bash -c "
      set -euo pipefail
      export PATH=/usr/local/cargo/bin:\$PATH CARGO_HOME=/target/cargo-home CARGO_TARGET_DIR=/target RUSTUP_HOME=/usr/local/rustup
      export WIRECATCH_TEST_FFMPEG=\$(command -v ffmpeg) WIRECATCH_REQUIRE_FFMPEG=1
      cd /work
      node scripts/gate.mjs --install-tools
      node scripts/gate.mjs --no-desktop --no-fail-fast rust
    "
  '
