#!/bin/bash
set -e

# Configuration
APP_NAME="ytm-hybrid"
VERSION="0.2.0"
MAINTAINER="Egehan Şımarık <egehan_smrk@github.com>"
COPYRIGHT="2026, Egehan Şımarık"
DESCRIPTION="Native YouTube Music Desktop Client built with Rust & GPUI."

# Directories
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="$PROJECT_DIR/dist"
APP_DIR="$DIST_DIR/AppDir"
BUILD_DIR="$PROJECT_DIR/target/release"

echo "=== YTM Hybrid Multi-Platform Packaging ==="
echo "Version: $VERSION"
echo ""

# Create dist directory
mkdir -p "$DIST_DIR"

# ============================================================================
# 1. Build Release Executable (if not already built)
# ============================================================================
echo "[1/5] Ensuring release build..."
if [ ! -f "$BUILD_DIR/ytm-hybrid" ] && [ ! -f "$BUILD_DIR/ytm-hybrid.exe" ]; then
    echo "  Running cargo build --release..."
    cd "$PROJECT_DIR"
    cargo build --release
fi

# ============================================================================
# 2. Portable .tar.gz Archive (Linux)
# ============================================================================
echo "[2/5] Creating portable .tar.gz archive..."

TAR_DIST="$DIST_DIR/ytm-hybrid-linux-x64"
rm -rf "$TAR_DIST"
mkdir -p "$TAR_DIST"

# Copy binary
if [ -f "$BUILD_DIR/ytm-hybrid" ]; then
    cp "$BUILD_DIR/ytm-hybrid" "$TAR_DIST/"
elif [ -f "$BUILD_DIR/ytm-hybrid.exe" ]; then
    # Windows build for testing
    cp "$BUILD_DIR/ytm-hybrid.exe" "$TAR_DIST/"
fi

# Copy icon
cp "$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png" "$TAR_DIST/"

# Copy LICENSE
cp "$PROJECT_DIR/LICENSE" "$TAR_DIST/"

# Create archive
cd "$DIST_DIR"
tar -czf "ytm-hybrid-v${VERSION}-linux-x64.tar.gz" ytm-hybrid-linux-x64
echo "  Created: dist/ytm-hybrid-v${VERSION}-linux-x64.tar.gz"

# ============================================================================
# 3. .deb Package (Linux)
# ============================================================================
echo "[3/5] Creating .deb package..."

# Update Cargo.toml with deb metadata
DEB_CONFIG='[package.metadata.deb]
name = "ytm-hybrid"
maintainer = "'"${MAINTAINER}"'"
copyright = "'"${COPYRIGHT}"'"
license-file = ["LICENSE", "444"]
extended-description = "'"${DESCRIPTION}"'"
section = "sound"
priority = "optional"
assets = [
    ["target/release/ytm-hybrid", "usr/bin/ytm-hybrid", "755"],
    ["assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png", "usr/share/icons/hicolor/256x256/apps/ytm-hybrid.png", "644"],
    ["assets/linux/ytm-hybrid.desktop", "usr/share/applications/ytm-hybrid.desktop", "644"],
]
depends = ["$auto"]'

# Write deb config (we'll handle this via cargo-deb config)
cd "$PROJECT_DIR"
cargo install cargo-deb --locked 2>/dev/null || true
cargo deb
echo "  Created: target/debian/ytm-hybrid_${VERSION}_amd64.deb"

# ============================================================================
# 4. .AppImage Package (Linux)
# ============================================================================
echo "[4/5] Creating .AppImage package..."

# Clean AppDir
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR"

# Copy binary
cp "$BUILD_DIR/ytm-hybrid" "$APP_DIR/ytm-hybrid"

# Make executable
chmod +x "$APP_DIR/ytm-hybrid"

# Copy icon
cp "$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png" "$APP_DIR/ytm-hybrid.png"

# Copy desktop file
cp "$PROJECT_DIR/assets/linux/ytm-hybrid.desktop" "$APP_DIR/ytm-hybrid.desktop"

# Create AppRun script
cat > "$APP_DIR/AppRun" << 'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/ytm-hybrid" "$@"
EOF
chmod +x "$APP_DIR/AppRun"

# Generate AppImage using appimagetool
APPIMAGE_OUT="$DIST_DIR/YTM_Hybrid-v${VERSION}-x86_64.AppImage"
if command -v appimagetool &> /dev/null; then
    appimagetool --compact --no-appstream "$APP_DIR" "$APPIMAGE_OUT"
    echo "  Created: $APPIMAGE_OUT"
elif command -v cargo-appimage &> /dev/null; then
    cd "$PROJECT_DIR"
    cargo appimage
else
    echo "  Warning: No AppImage tool found (install appimagetool or cargo-appimage)"
fi

# ============================================================================
# 5. .dmg Package (macOS)
# ============================================================================
echo "[5/5] Creating .dmg installer..."

# Check if we're on macOS
if [ "$(uname)" != "Darwin" ]; then
    echo "  Warning: macOS packaging requires a Mac build environment"
    echo "  Use cargo-bundle or create-dmg on macOS"
else
    # Build macOS bundle
    cd "$PROJECT_DIR"
    cargo build --release --target=x86_64-apple-darwin

    # Install create-dmg if not available
    if ! command -v create-dmg &> /dev/null; then
        echo "  Installing create-dmg..."
        gem install create-dmg 2>/dev/null || true
    fi

    # Create DMG
    create-dmg \
        --volname "YTM Hybrid ${VERSION}" \
        --background "$PROJECT_DIR/assets/macos/background.png" \
        --window-pos 200 120 \
        --window-size 600 400 \
        --icon-size 80 \
        --icon "YTM Hybrid.app" 180 190 \
        --app-drop-link 420 190 \
        "$APPIMAGE_OUT" \
        "$PROJECT_DIR/target/x86_64-apple-darwin/release/YTM Hybrid.app"

    echo "  Created: dist/YTM_Hybrid-v${VERSION}-x64.dmg"
fi

echo ""
echo "=== Packaging Complete ==="
echo "Output files in dist/:"
ls -la "$DIST_DIR"/*
