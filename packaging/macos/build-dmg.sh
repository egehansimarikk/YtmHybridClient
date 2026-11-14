#!/bin/bash
# macOS .app Bundle and .dmg Builder for YTM Hybrid
# Requires: macOS, iconutil, create-dmg (brew install create-dmg)

set -e

APP_NAME="YTM Hybrid"
BUNDLE_ID="com.egehansimarik.ytm-hybrid"
VERSION="0.4.2"
EXECUTABLE="ytm-hybrid"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_DIR/target/release"
DIST_DIR="$PROJECT_DIR/dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"

echo "=== YTM Hybrid macOS Packaging ==="
echo "Version: $VERSION"
echo ""

# Check if running on macOS
if [ "$(uname)" != "Darwin" ]; then
    echo "Error: This script must be run on macOS"
    exit 1
fi

# Check if binary exists
if [ ! -f "$BUILD_DIR/$EXECUTABLE" ]; then
    echo "Error: Release binary not found at $BUILD_DIR/$EXECUTABLE"
    echo "Please run: cargo build --release"
    exit 1
fi

# Clean and create directories
echo "[1/6] Creating .app bundle structure..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy binary
echo "[2/6] Copying executable..."
cp "$BUILD_DIR/$EXECUTABLE" "$APP_DIR/Contents/MacOS/"
chmod +x "$APP_DIR/Contents/MacOS/$EXECUTABLE"

# Copy Info.plist
echo "[3/6] Installing Info.plist..."
cp "$SCRIPT_DIR/Info.plist" "$APP_DIR/Contents/"

# Convert PNG to ICNS
echo "[4/6] Creating app icon (AppIcon.icns)..."
ICON_SOURCE="$PROJECT_DIR/assets/linux/icons/hicolor/256x256/apps/ytm-hybrid.png"
ICONSET_DIR="$DIST_DIR/AppIcon.iconset"

if [ ! -f "$ICON_SOURCE" ]; then
    echo "Warning: Icon not found at $ICON_SOURCE"
    echo "Skipping icon creation..."
else
    # Create iconset directory
    rm -rf "$ICONSET_DIR"
    mkdir -p "$ICONSET_DIR"

    # Generate required icon sizes
    # macOS requires: 16x16, 32x32, 128x128, 256x256, 512x512, 1024x1024
    # Each at 1x and 2x (@2x) resolution

    for size in 16 32 128 256 512; do
        size2x=$((size * 2))
        sips -z $size $size "$ICON_SOURCE" --out "$ICONSET_DIR/icon_${size}x${size}.png" >/dev/null 2>&1
        sips -z $size2x $size2x "$ICON_SOURCE" --out "$ICONSET_DIR/icon_${size}x${size}@2x.png" >/dev/null 2>&1
    done

    # Special case: 1024x1024 (no @2x version)
    sips -z 1024 1024 "$ICON_SOURCE" --out "$ICONSET_DIR/icon_512x512@2x.png" >/dev/null 2>&1

    # Convert iconset to icns
    iconutil -c icns "$ICONSET_DIR" -o "$APP_DIR/Contents/Resources/AppIcon.icns"
    rm -rf "$ICONSET_DIR"

    echo "  ✓ AppIcon.icns created"
fi

# Code signing (optional, if developer certificate available)
echo "[5/6] Code signing..."
if security find-identity -v -p codesigning | grep -q "Developer ID Application"; then
    echo "  Found Developer ID, signing..."
    codesign --force --deep --sign "Developer ID Application" "$APP_DIR"
    echo "  ✓ Signed"
else
    echo "  ⚠ No Developer ID found, creating ad-hoc signature..."
    codesign --force --deep --sign - "$APP_DIR"
    echo "  ✓ Ad-hoc signature applied (app will run locally only)"
fi

# Create DMG
echo "[6/6] Creating .dmg installer..."
DMG_NAME="YTM-Hybrid-${VERSION}-macOS"
DMG_PATH="$DIST_DIR/${DMG_NAME}.dmg"

# Remove old DMG
rm -f "$DMG_PATH"

# Check if create-dmg is available
if ! command -v create-dmg &> /dev/null; then
    echo "  Warning: create-dmg not found"
    echo "  Install with: brew install create-dmg"
    echo ""
    echo "  Creating simple DMG with hdiutil instead..."

    # Fallback: create simple DMG with hdiutil
    TMP_DMG="$DIST_DIR/tmp.dmg"
    hdiutil create -volname "$APP_NAME" -srcfolder "$APP_DIR" -ov -format UDZO "$TMP_DMG"
    mv "$TMP_DMG" "$DMG_PATH"
else
    # Use create-dmg for better DMG
    create-dmg \
        --volname "$APP_NAME $VERSION" \
        --volicon "$APP_DIR/Contents/Resources/AppIcon.icns" \
        --window-pos 200 120 \
        --window-size 600 400 \
        --icon-size 100 \
        --icon "$APP_NAME.app" 175 190 \
        --hide-extension "$APP_NAME.app" \
        --app-drop-link 425 190 \
        --no-internet-enable \
        "$DMG_PATH" \
        "$APP_DIR" 2>/dev/null || true

    # Fallback if create-dmg fails
    if [ ! -f "$DMG_PATH" ]; then
        echo "  create-dmg failed, falling back to hdiutil..."
        hdiutil create -volname "$APP_NAME" -srcfolder "$APP_DIR" -ov -format UDZO "$DMG_PATH"
    fi
fi

echo ""
echo "=== macOS Packaging Complete ==="
echo ""
echo "Created files:"
echo "  • App bundle: $APP_DIR"
echo "  • DMG installer: $DMG_PATH"
echo ""
echo "To test locally:"
echo "  1. Open $DMG_PATH"
echo "  2. Drag YTM Hybrid.app to Applications"
echo "  3. First launch: Right-click → Open (to bypass Gatekeeper)"
echo ""
echo "Note: For distribution outside the Mac App Store, you need:"
echo "  • Developer ID certificate from Apple"
echo "  • Notarization (xcrun notarytool)"
echo ""
