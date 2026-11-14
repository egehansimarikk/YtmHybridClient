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

# Çalışan eski işlemi kapat
if pgrep -x "$APP_NAME" > /dev/null; then
    echo "Stopping running $APP_NAME process..."
    pkill -x "$APP_NAME" || true
    sleep 1
fi

mkdir -p "$BIN_DIR" "$ICON_DIR" "$DESKTOP_DIR"

# Eski sürümlerden kalan dosya ve klasörleri temizle
echo "Cleaning up old installation files..."
rm -f "$BIN_DIR/$APP_NAME"
rm -rf "$HOME/.local/share/ytm-hybrid"
rm -f "$ICON_DIR/$APP_NAME.png"
rm -f "$DESKTOP_DIR/$APP_NAME.desktop" "$DESKTOP_DIR/YTM Hybrid.desktop"

# Yeni binary ve ikonu kopyala
echo "Installing binary and icon..."
cp "$BINARY_SRC" "$BIN_DIR/$APP_NAME"
chmod +x "$BIN_DIR/$APP_NAME"
cp "$ICON_SRC" "$ICON_DIR/$APP_NAME.png"

# Tam yol (Exec=$BIN_DIR/$APP_NAME) ile masaüstü kısayolu oluştur
cat > "$DESKTOP_DIR/$APP_NAME.desktop" << DESKTOP_EOF
[Desktop Entry]
Type=Application
Name=YTM Hybrid
GenericName=Music Player
Comment=YouTube Music streaming client, built with Rust and GPUI
Icon=$ICON_DIR/$APP_NAME.png
Exec=$BIN_DIR/$APP_NAME %U
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

# Sistem genelinde (/usr/bin) çakışan başka bir kurulum varsa uyar
if [ -f "/usr/bin/$APP_NAME" ]; then
    echo -e "${YELLOW}Warning: /usr/bin/$APP_NAME exists and may override ~/.local/bin/$APP_NAME.${NC}"
fi

echo -e "${GREEN}✓ Installation / Update to v${VERSION} complete!${NC}"