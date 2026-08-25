#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev rustup

export PATH="$HOME/.cargo/bin:$PATH"
if ! rustup toolchain list | grep -q '^stable'; then
  rustup toolchain install stable --profile minimal
fi
rustup default stable

if ! command -v pnpm >/dev/null 2>&1; then
  corepack enable
  corepack prepare pnpm@11 --activate
fi

printf '\nEnvironment ready:\n'
python3 --version
node --version
pnpm --version
rustc --version
cargo --version
