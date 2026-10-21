#!/bin/bash
set -e

# Configuration
APP_NAME="ytm-hybrid"
VERSION="0.1.0"
MAINTAINER="Egehan Şımarık <egehan_smrk@github.com>"
COPYRIGHT="2026, Egehan Şımarık"
DESCRIPTION="Native YouTube Music Desktop Client built with Rust & GPUI."

# Directories
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$PROJECT_DIR/dist"
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

# Check platform
case "$(uname -s)" in
    Linux*)
        BINARY_NAME="ytm-hybrid"
        ;;
    Darwin*)
        BINARY_NAME="ytm-hybrid"
        ;;
    MINGW*|MSYS*)
        BINARY_NAME="ytm-hybrid.exe"
        ;;
    *)
        echo "Unsupported platform: $(uname -s)"
        exit 1
        ;;
esac

if [ ! -f "$BUILD_DIR/$BINARY_NAME" ]; then
    echo "  Running cargo build --release..."
    cd "$PROJECT_DIR"
    cargo build --release
fi

# Verify binary exists
if [ ! -f "$BUILD_DIR/$BINARY_NAME" ]; then
    echo "  ERROR: Binary not found at $BUILD_DIR/$BINARY_NAME"
    exit 1
fi
echo "  Found binary: $BUILD_DIR/$BINARY_NAME"

# ============================================================================
# 2. Portable .tar.gz Archive (Linux)
# ============================================================================
echo "[2/5] Creating portable .tar.gz archive..."

TAR_DIST="$DIST_DIR/ytm-hybrid-linux-x64"
rm -rf "$TAR_DIST"
mkdir -p "$TAR_DIST"

# Copy binary
cp "$BUILD_DIR/$BINARY_NAME" "$TAR_DIST/"

# Copy icon
cp "$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png" "$TAR_DIST/ytm-hybrid.png"

# Copy LICENSE
cp "$PROJECT_DIR/LICENSE" "$TAR_DIST/"

# Create README
cat > "$TAR_DIST/README.md" << EOF
# YTM Hybrid v${VERSION}

Native YouTube Music Desktop Client built with Rust & GPUI.

## Installation

### Linux (tar.gz)
\`\`\`bash
tar -xzf ytm-hybrid-v${VERSION}-linux-x64.tar.gz
cd ytm-hybrid-linux-x64
./ytm-hybrid
\`\`\`

### Debian/Ubuntu
\`\`\`bash
sudo dpkg -i ytm-hybrid_${VERSION}_amd64.deb
sudo apt-get install -f  # Fix dependencies
\`\`\`

## Features
- Native YouTube Music client
- System tray integration
- Media key support
- Notifications
- Lyrics display

## License
GPL-3.0-or-later
EOF

# Create archive
cd "$DIST_DIR"
tar -czf "ytm-hybrid-v${VERSION}-linux-x64.tar.gz" ytm-hybrid-linux-x64
echo "  Created: dist/ytm-hybrid-v${VERSION}-linux-x64.tar.gz"

# ============================================================================
# 3. .deb Package (Linux)
# ============================================================================
echo "[3/5] Creating .deb package..."

# Check if cargo-deb is installed
if ! command -v cargo-deb &> /dev/null; then
    echo "  Installing cargo-deb..."
    cargo install cargo-deb --locked
fi

# Build .deb package
cd "$PROJECT_DIR"
cargo deb --no-strip
echo "  Created: target/debian/ytm-hybrid_${VERSION}_amd64.deb"

# ============================================================================
# 4. .AppImage Package (Linux)
# ============================================================================
echo "[4/5] Creating .AppImage package..."

# Check if appimagetool is installed
if command -v appimagetool &> /dev/null; then
    APP_DIR="$DIST_DIR/AppDir"
    rm -rf "$APP_DIR"
    mkdir -p "$APP_DIR/usr/bin"
    mkdir -p "$APP_DIR/usr/share/applications"
    mkdir -p "$APP_DIR/usr/share/icons/hicolor/256x256/apps"

    # Copy binary
    cp "$BUILD_DIR/ytm-hybrid" "$APP_DIR/usr/bin/ytm-hybrid"
    chmod +x "$APP_DIR/usr/bin/ytm-hybrid"

    # Copy icon
    cp "$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png" "$APP_DIR/usr/share/icons/hicolor/256x256/apps/ytm-hybrid.png"

    # Copy desktop file
    cp "$PROJECT_DIR/assets/linux/ytm-hybrid.desktop" "$APP_DIR/usr/share/applications/ytm-hybrid.desktop"

    # Create AppRun script
    cat > "$APP_DIR/AppRun" << 'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/ytm-hybrid" "$@"
EOF
    chmod +x "$APP_DIR/AppRun"

    # Generate AppImage using appimagetool
    APPIMAGE_OUT="$DIST_DIR/YTM_Hybrid-v${VERSION}-x86_64.AppImage"
    appimagetool --compact --no-appstream "$APP_DIR" "$APPIMAGE_OUT"
    echo "  Created: $APPIMAGE_OUT"
else
    echo "  Warning: appimagetool not found (install with: cargo install appimagetool)"
    echo "  Skipping AppImage generation."
fi

# ============================================================================
# 5. .dmg Package (macOS)
# ============================================================================
echo "[5/5] Creating .dmg installer..."

# Check if we're on macOS
if [ "$(uname)" = "Darwin" ]; then
    # Build macOS bundle
    cd "$PROJECT_DIR"

    # Install cargo-bundle if not available
    if ! command -v cargo-bundle &> /dev/null; then
        echo "  Installing cargo-bundle..."
        cargo install cargo-bundle --locked
    fi

    # Build with bundling
    cargo bundle --release

    # Create DMG using create-dmg
    if command -v create-dmg &> /dev/null; then
        DMG_OUT="$DIST_DIR/YTM_Hybrid-v${VERSION}-x64.dmg"

        # Create background image if it doesn't exist
        if [ ! -f "$PROJECT_DIR/assets/macos/background.png" ]; then
            echo "  Warning: No background image found at assets/macos/background.png"
        fi

        create-dmg \
            --volname "YTM Hybrid ${VERSION}" \
            --window-pos 200 120 \
            --window-size 600 400 \
            --icon-size 80 \
            --icon "YTM Hybrid.app" 180 190 \
            --app-drop-link 420 190 \
            "$DMG_OUT" \
            "$PROJECT_DIR/target/x86_64-apple-darwin/release/YTM Hybrid.app"

        echo "  Created: $DMG_OUT"
    else
        echo "  Warning: create-dmg not found (install with: brew install create-dmg)"
        echo "  You can manually create a DMG from the bundled app."
    fi
else
    echo "  Note: macOS packaging requires a Mac build environment."
    echo "  To create .dmg on macOS:"
    echo "    brew install create-dmg"
    echo "    cargo bundle --release"
    echo "    create-dmg dist/YTM_Hybrid*.app dist/YTM_Hybrid-v${VERSION}-x64.dmg"
fi

echo ""
echo "=== Packaging Complete ==="
echo "Output files:"
ls -la "$DIST_DIR"/*
