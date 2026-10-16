#!/usr/bin/env bash
# Builds the native tab-list plugin *inside* Pumpkin's workspace so it uses the
# server's own Cargo.lock (avoiding pre-release dependency mismatches such as
# rsa/pkcs1) and the exact same `pumpkin` crate (guaranteeing ABI compatibility).
#
# Usage:  bash native-potions/build-in-pumpkin.sh [workdir]
set -euo pipefail

# rustup installs cargo into ~/.cargo/bin; non-interactive SSH shells don't source it.
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"

TAG="0.2.0+26.3-26.51"
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="${1:-/tmp/pumpkin-native}"

echo ">> Cloning Pumpkin @ ${TAG} into ${WORK}"
rm -rf "$WORK"
git clone --depth 1 --branch "$TAG" https://github.com/Pumpkin-MC/Pumpkin "$WORK"

echo ">> Injecting native-potions into the workspace"
mkdir -p "$WORK/native-potions/src"
cp "$HERE/src/lib.rs" "$WORK/native-potions/src/lib.rs"
cat > "$WORK/native-potions/Cargo.toml" <<'EOF'
[package]
name = "native_potions"
version = "0.1.0"
edition = "2024"
description = "Native tab list companion for Lazarus"

[lib]
crate-type = ["cdylib"]

[dependencies]
pumpkin = { path = "../crates/pumpkin" }
pumpkin-api-macros = { path = "../crates/pumpkin-api-macros" }
pumpkin-data = { path = "../crates/pumpkin-data" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tracing = "0.1"
EOF

# Register as a workspace member (idempotent). Anchor the match to the members
# list line so it does NOT touch `pumpkin-world = { path = "crates/pumpkin-world", ... }`.
if ! grep -q '"native-potions"' "$WORK/Cargo.toml"; then
    sed -i 's#^    "crates/pumpkin-world",$#    "crates/pumpkin-world",\n    "native-potions",#' "$WORK/Cargo.toml"
fi

echo ">> Building (this is heavy: ~700 crates, first time 20-40 min)"
cd "$WORK"
# Build for the host explicitly (avoids any target inherited from a parent .cargo config).
HOST_TRIPLE="$(rustc -vV | awk '/^host:/{print $2}')"
RUST_MIN_STACK=134217728 cargo build --release -p native_potions --target "$HOST_TRIPLE"

echo
echo ">> Done: $WORK/target/$HOST_TRIPLE/release/libnative_potions.so"
echo ">> Copy it to the server:  cp $WORK/target/$HOST_TRIPLE/release/libnative_potions.so /ruta/plugins/native_potions.so"
