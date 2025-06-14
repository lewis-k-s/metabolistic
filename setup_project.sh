#!/bin/bash
set -e

echo "Setting up Metabolistic3D project in headless environment..."

# Install ALSA development libraries and other audio dependencies
echo "Installing audio system dependencies..."
apt-get update && apt-get install -y \
    libasound2-dev \
    pkg-config \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*

# Set environment variables for audio libraries and pkg-config
export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig
export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1
export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1

# Always use headless features since this is for codex environment
export CARGO_FEATURES="--no-default-features --features headless"

echo "Environment setup:"
echo "  - PKG_CONFIG_PATH: $PKG_CONFIG_PATH"
echo "  - Rust version: ${CODEX_ENV_RUST_VERSION:-default}"
echo "  - Build features: $CARGO_FEATURES"

# Check if Cargo.toml exists
if [ ! -f "Cargo.toml" ]; then
    echo "Error: Cargo.toml not found. Make sure you're in the project root directory."
    exit 1
fi

# Verify pkg-config can find alsa
echo "Checking for ALSA library..."
if pkg-config --exists alsa; then
    echo "ALSA library found"
    pkg-config --modversion alsa
else
    echo "!! ALSA library not found, but proceeding with headless build"
fi

echo "Running cargo check with headless features..."
cargo check $CARGO_FEATURES

echo "✅ Headless build successful!"
echo "Project setup complete!"