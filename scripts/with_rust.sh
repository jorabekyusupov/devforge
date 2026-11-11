#!/bin/sh
# Use a standard toolchain when present; otherwise the isolated M1 toolchain.
set -eu
if command -v cargo >/dev/null 2>&1; then
  exec "$@"
fi
devforge_tools="${DEVFORGE_TOOLCHAIN_DIR:-$HOME/Library/Caches/devforge/toolchains}"
if [ ! -x "$devforge_tools/cargo/bin/cargo" ]; then
  echo 'Install the Rust toolchain documented in README.md.' >&2
  exit 1
fi
export CARGO_HOME="$devforge_tools/cargo"
export RUSTUP_HOME="$devforge_tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
exec "$@"
