#!/bin/bash
set -e

# Configuration
APP_NAME="ytm-hybrid"
VERSION="0.4.2"
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
# 2. Portable .tar.gz Archive (Linux) with Automated Installer
# ============================================================================
echo "[2/5] Creating portable .tar.gz archive with automated installer..."

TAR_DIST="$DIST_DIR/ytm-hybrid-linux-x64"
rm -rf "$TAR_DIST"
mkdir -p "$TAR_DIST"

# Copy binary
cp "$BUILD_DIR/$BINARY_NAME" "$TAR_DIST/"

# Copy icon
ICON_SRC="$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png"
if [ -f "$ICON_SRC" ]; then
    cp "$ICON_SRC" "$TAR_DIST/ytm-hybrid.png"
fi

# Copy LICENSE
if [ -f "$PROJECT_DIR/LICENSE" ]; then
    cp "$PROJECT_DIR/LICENSE" "$TAR_DIST/"
fi

# Embed the intelligent install.sh script inside the tar.gz bundle
cat > "$TAR_DIST/install.sh" << 'INSTALL_EOF'
#!/bin/bash
# YTM Hybrid Linux Installation & Update Script
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_NAME="ytm-hybrid"
VERSION="0.4.2"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

BIN_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
DESKTOP_DIR="$HOME/.local/share/applications"

if [ -f "$SCRIPT_DIR/$APP_NAME" ]; then
    BINARY_SRC="$SCRIPT_DIR/$APP_NAME"
else
    echo -e "${RED}Error: $APP_NAME binary not found${NC}"
    exit 1
fi

if [ -f "$SCRIPT_DIR/$APP_NAME.png" ]; then
    ICON_SRC="$SCRIPT_DIR/$APP_NAME.png"
else
    echo -e "${RED}Error: $APP_NAME.png icon not found${NC}"
    exit 1
fi

echo -e "${GREEN}Installing / Updating YTM Hybrid v${VERSION}...${NC}"

# Stop running instances to prevent file locks / text file busy errors
if pgrep -x "$APP_NAME" > /dev/null; then
    echo "Stopping running $APP_NAME process..."
    pkill -x "$APP_NAME" || true
    sleep 1
fi

mkdir -p "$BIN_DIR"
mkdir -p "$ICON_DIR"
mkdir -p "$DESKTOP_DIR"

# Clean overwrite binary and icon
echo "Installing binary and icon..."
rm -f "$BIN_DIR/$APP_NAME"
cp "$BINARY_SRC" "$BIN_DIR/$APP_NAME"
chmod +x "$BIN_DIR/$APP_NAME"

rm -f "$ICON_DIR/$APP_NAME.png"
cp "$ICON_SRC" "$ICON_DIR/$APP_NAME.png"

# Create desktop entry
cat > "$DESKTOP_DIR/$APP_NAME.desktop" << 'DESKTOP_EOF'
[Desktop Entry]
Type=Application
Name=YTM Hybrid
GenericName=Music Player
Comment=YouTube Music streaming client, built with Rust and GPUI
Icon=ytm-hybrid
Exec=ytm-hybrid %U
Terminal=false
Categories=AudioVideo;Audio;Player;
Keywords=music;youtube;audio;player;youtube music;
MimeType=x-scheme-handler/youtube;audio/mpeg;audio/flac;audio/mp4;audio/aac;audio/ogg;audio/opus;audio/x-wav;audio/x-aiff;audio/webm;audio/x-matroska;audio/x-wavpack;audio/x-ape;
StartupWMClass=ytm-hybrid
StartupNotify=true
DESKTOP_EOF

if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
fi

echo -e "${GREEN}✓ Installation / Update to v${VERSION} complete!${NC}"
echo "You can launch YTM Hybrid from your application menu or by typing 'ytm-hybrid' in terminal."
INSTALL_EOF

chmod +x "$TAR_DIST/install.sh"

# Create README
cat > "$TAR_DIST/README.md" << EOF
# YTM Hybrid v${VERSION}

Native YouTube Music Desktop Client built with Rust & GPUI.

## Installation

### Linux (Automated Installer)
\`\`\`bash
tar -xzf ytm-hybrid-v${VERSION}-linux-x64.tar.gz
cd ytm-hybrid-linux-x64
./install.sh
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

if ! command -v cargo-deb &> /dev/null; then
    echo "  Installing cargo-deb..."
    cargo install cargo-deb --locked
fi

cd "$PROJECT_DIR"
cargo deb --no-strip
echo "  Created: target/debian/ytm-hybrid_${VERSION}_amd64.deb"

# ============================================================================
# 4. .AppImage Package (Linux)
# ============================================================================
echo "[4/5] Creating .AppImage package..."

if command -v appimagetool &> /dev/null; then
    APP_DIR="$DIST_DIR/AppDir"
    rm -rf "$APP_DIR"
    mkdir -p "$APP_DIR/usr/bin"
    mkdir -p "$APP_DIR/usr/share/applications"
    mkdir -p "$APP_DIR/usr/share/icons/hicolor/256x256/apps"

    cp "$BUILD_DIR/ytm-hybrid" "$APP_DIR/usr/bin/ytm-hybrid"
    chmod +x "$APP_DIR/usr/bin/ytm-hybrid"

    cp "$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png" "$APP_DIR/usr/share/icons/hicolor/256x256/apps/ytm-hybrid.png"
    cp "$PROJECT_DIR/assets/linux/ytm-hybrid.desktop" "$APP_DIR/usr/share/applications/ytm-hybrid.desktop"

    cat > "$APP_DIR/AppRun" << 'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/ytm-hybrid" "$@"
EOF
    chmod +x "$APP_DIR/AppRun"

    APPIMAGE_OUT="$DIST_DIR/YTM_Hybrid-v${VERSION}-x86_64.AppImage"
    appimagetool --compact --no-appstream "$APP_DIR" "$APPIMAGE_OUT"
    echo "  Created: $APPIMAGE_OUT"
else
    echo "  Warning: appimagetool not found. Skipping AppImage generation."
fi

# ============================================================================
# 5. .dmg Package (macOS)
# ============================================================================
echo "[5/5] Creating .dmg installer..."

if [ "$(uname)" = "Darwin" ]; then
    cd "$PROJECT_DIR"

    if ! command -v cargo-bundle &> /dev/null; then
        echo "  Installing cargo-bundle..."
        cargo install cargo-bundle --locked
    fi

    cargo bundle --release

    if command -v create-dmg &> /dev/null; then
        DMG_OUT="$DIST_DIR/YTM_Hybrid-v${VERSION}-x64.dmg"
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
        echo "  Warning: create-dmg not found."
    fi
else
    echo "  Note: macOS packaging requires a Mac build environment."
fi

echo ""
echo "=== Packaging Complete ==="
echo "Output files:"
ls -la "$DIST_DIR"/*