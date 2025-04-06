#!/bin/bash

# Get agave-geyser-plugin-interface version from Cargo.lock
AGAVE_VERSION=$(grep -A 1 'name = "agave-geyser-plugin-interface"' Cargo.lock | grep version | cut -d'"' -f2)
echo "Found agave-geyser-plugin-interface version: $AGAVE_VERSION"

# Get Agave's rust-toolchain.toml version based on the agave-geyser-plugin-interface version
VERSION=$(curl -s "https://raw.githubusercontent.com/anza-xyz/agave/v$AGAVE_VERSION/rust-toolchain.toml" | grep channel | cut -d'"' -f2)
echo "Found Agave's rust-toolchain.toml version: $VERSION"

# Check current version
CURRENT_VERSION=$(grep channel rust-toolchain.toml | cut -d'"' -f2)
echo "Current rust-toolchain.toml version: $CURRENT_VERSION"

if [ "$CURRENT_VERSION" != "$VERSION" ]; then
    echo "Updating rust-toolchain.toml to $VERSION"
    sed -i '' "s/channel = \".*\"/channel = \"$VERSION\"/" rust-toolchain.toml
else
    echo "rust-toolchain.toml is already up to date"
fi
